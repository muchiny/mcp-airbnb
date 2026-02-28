use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ProtocolVersion, RawResource, RawResourceTemplate,
        ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
        ResourceTemplate, ServerCapabilities, ServerInfo,
    },
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};

use crate::domain::analytics;
use crate::domain::search_params::SearchParams;
use crate::ports::airbnb_client::AirbnbClient;

// ---------- Resource Store ----------

/// Thread-safe store of fetched Airbnb data exposed as MCP resources.
/// Keys are URIs like `airbnb://listing/12345`, values are text content.
#[derive(Clone, Default)]
pub struct ResourceStore {
    entries: Arc<RwLock<HashMap<String, ResourceEntry>>>,
}

#[derive(Clone)]
struct ResourceEntry {
    name: String,
    text: String,
}

impl ResourceStore {
    async fn insert(&self, uri: impl Into<String>, name: impl Into<String>, text: String) {
        self.entries.write().await.insert(
            uri.into(),
            ResourceEntry {
                name: name.into(),
                text,
            },
        );
    }

    async fn get(&self, uri: &str) -> Option<ResourceEntry> {
        self.entries.read().await.get(uri).cloned()
    }

    async fn list(&self) -> Vec<(String, String)> {
        self.entries
            .read()
            .await
            .iter()
            .map(|(uri, entry)| (uri.clone(), entry.name.clone()))
            .collect()
    }
}

impl std::fmt::Debug for ResourceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceStore").finish()
    }
}

// ---------- Tool parameter types ----------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchToolParams {
    /// Location to search (e.g. "Paris, France", "Tokyo", "New York", "Porto-Vecchio, Corsica")
    pub location: String,
    /// Check-in date (YYYY-MM-DD format). Must be paired with checkout.
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD format). Must be paired with checkin.
    pub checkout: Option<String>,
    /// Number of adult guests (default: 1 if omitted)
    pub adults: Option<u32>,
    /// Number of children
    pub children: Option<u32>,
    /// Number of infants
    pub infants: Option<u32>,
    /// Number of pets
    pub pets: Option<u32>,
    /// Minimum price per night in the listing's local currency
    pub min_price: Option<u32>,
    /// Maximum price per night in the listing's local currency
    pub max_price: Option<u32>,
    /// Property type filter (e.g. "Entire home", "Private room", "Hotel room", "Shared room")
    pub property_type: Option<String>,
    /// Pagination cursor from previous search results. Pass this to load the next page.
    pub cursor: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DetailToolParams {
    /// Airbnb listing ID (numeric string from search results, e.g. "12345678")
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReviewsToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Pagination cursor from previous results
    pub cursor: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CalendarToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Number of months to fetch (1-12, default: 3)
    pub months: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HostProfileToolParams {
    /// Airbnb listing ID to get host profile from
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NeighborhoodStatsToolParams {
    /// Location to analyze (e.g. "Paris, France", "Brooklyn, NY"). Use the same format as `airbnb_search`.
    pub location: String,
    /// Check-in date (YYYY-MM-DD format). Filters listings available on these dates.
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD format). Filters listings available on these dates.
    pub checkout: Option<String>,
    /// Property type filter (e.g. "Entire home", "Private room", "Hotel room", "Shared room")
    pub property_type: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OccupancyEstimateToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Number of months to analyze (1-12, default: 3)
    pub months: Option<u32>,
}

// ---------- New analytical tool params ----------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CompareListingsToolParams {
    /// List of Airbnb listing IDs to compare (2-10 for detailed comparison).
    /// If omitted, provide `location` to auto-discover listings for market-scale comparison.
    pub ids: Option<Vec<String>>,
    /// Location to auto-discover listings (e.g. "Paris, France"). Used when `ids` is omitted.
    /// Fetches up to `max_listings` from search results for market-scale comparison.
    pub location: Option<String>,
    /// Maximum number of listings when using location-based discovery (default: 20, max: 100)
    pub max_listings: Option<u32>,
    /// Check-in date (YYYY-MM-DD) for location search
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD) for location search
    pub checkout: Option<String>,
    /// Property type filter for location search
    pub property_type: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PriceTrendsToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Number of months to analyze (1-12, default: 12)
    pub months: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GapFinderToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Number of months to analyze (1-12, default: 3)
    pub months: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RevenueEstimateToolParams {
    /// Airbnb listing ID. If provided, uses the listing's actual calendar and neighborhood data.
    pub id: Option<String>,
    /// Location for neighborhood comparison (required if `id` is not provided)
    pub location: Option<String>,
    /// Number of months to project (1-12, default: 12)
    pub months: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListingScoreToolParams {
    /// Airbnb listing ID to score
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AmenityAnalysisToolParams {
    /// Airbnb listing ID to analyze
    pub id: String,
    /// Location for neighborhood comparison. If omitted, uses the listing's location.
    pub location: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MarketComparisonToolParams {
    /// List of 2-5 locations to compare (e.g. `["Paris, France", "Barcelona, Spain"]`)
    pub locations: Vec<String>,
    /// Check-in date (YYYY-MM-DD)
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD)
    pub checkout: Option<String>,
    /// Property type filter
    pub property_type: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HostPortfolioToolParams {
    /// Airbnb listing ID to identify the host and analyze their portfolio
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewSentimentToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Maximum number of review pages to fetch (default: 5)
    #[schemars(range(min = 1, max = 20))]
    pub max_pages: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompetitivePositioningToolParams {
    /// Airbnb listing ID to analyze
    pub id: String,
    /// Location for neighborhood comparison. If omitted, uses the listing's location.
    #[schemars(description = "Location for neighborhood comparison")]
    pub location: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimalPricingToolParams {
    /// Airbnb listing ID to get pricing recommendation for
    pub id: String,
    /// Location for neighborhood comparison. If omitted, uses the listing's location.
    #[schemars(description = "Location for neighborhood comparison")]
    pub location: Option<String>,
}

// ---------- MCP Server ----------

/// Lightweight cache of listing prices discovered from search results.
/// Maps `listing_id` -> (`price_per_night`, currency).
#[derive(Clone, Default)]
struct PriceCache {
    prices: Arc<RwLock<HashMap<String, (f64, String)>>>,
}

impl PriceCache {
    async fn insert(&self, id: &str, price: f64, currency: &str) {
        if price > 0.0 {
            self.prices
                .write()
                .await
                .insert(id.to_string(), (price, currency.to_string()));
        }
    }

    async fn get(&self, id: &str) -> Option<(f64, String)> {
        self.prices.read().await.get(id).cloned()
    }
}

impl std::fmt::Debug for PriceCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PriceCache").finish()
    }
}

#[derive(Clone)]
pub struct AirbnbMcpServer {
    client: Arc<dyn AirbnbClient>,
    tool_router: ToolRouter<Self>,
    resources: ResourceStore,
    price_cache: PriceCache,
}

#[tool_router]
impl AirbnbMcpServer {
    pub fn new(client: Arc<dyn AirbnbClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
            resources: ResourceStore::default(),
            price_cache: PriceCache::default(),
        }
    }

    /// Get listing detail with price fallback from search cache.
    async fn get_detail_with_price(
        &self,
        id: &str,
    ) -> crate::error::Result<crate::domain::listing::ListingDetail> {
        let mut detail = self.client.get_listing_detail(id).await?;
        if detail.price_per_night == 0.0
            && let Some((price, currency)) = self.price_cache.get(id).await
        {
            detail.price_per_night = price;
            detail.currency = currency;
        }
        Ok(detail)
    }

    /// Search Airbnb listings by location, dates, and guest count.
    /// Returns a list of available listings matching the search criteria.
    #[tool(
        name = "airbnb_search",
        description = "Search Airbnb listings by location, dates, and guest count. Returns a list of available listings with prices, ratings, and links. Use this as the starting point to discover listings and get their IDs for other tools.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    #[allow(clippy::too_many_lines)]
    async fn airbnb_search(
        &self,
        Parameters(params): Parameters<SearchToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let search_params = SearchParams {
            location: params.location,
            checkin: params.checkin,
            checkout: params.checkout,
            adults: params.adults,
            children: params.children,
            infants: params.infants,
            pets: params.pets,
            min_price: params.min_price,
            max_price: params.max_price,
            property_type: params.property_type,
            cursor: params.cursor,
        };

        match self.client.search_listings(&search_params).await {
            Ok(result) => {
                let mut text = String::new();
                if result.listings.is_empty() {
                    text.push_str("No listings found for this search.\n");
                } else {
                    let _ = writeln!(text, "Found {} listings:\n", result.listings.len());
                    for (i, listing) in result.listings.iter().enumerate() {
                        let _ = write!(
                            text,
                            "{}. **{}** (ID: {})\n   {}\n   {}{}/night",
                            i + 1,
                            listing.name,
                            listing.id,
                            listing.location,
                            listing.currency,
                            listing.price_per_night,
                        );
                        if let Some(rating) = listing.rating {
                            let _ = write!(
                                text,
                                " | Rating: {rating:.1} ({} reviews)",
                                listing.review_count,
                            );
                        }
                        if let Some(ref pt) = listing.property_type {
                            let _ = write!(text, " | {pt}");
                        }
                        let _ = writeln!(text, "\n   {}\n", listing.url);
                    }
                    if let Some(ref cursor) = result.next_cursor {
                        let _ = writeln!(
                            text,
                            "More results available. Use cursor: \"{cursor}\" to get next page."
                        );
                    }
                }
                // Cache listing prices from search results for later use
                for listing in &result.listings {
                    self.price_cache
                        .insert(&listing.id, listing.price_per_night, &listing.currency)
                        .await;
                }
                let uri = format!("airbnb://search/{}", search_params.location);
                let name = format!("Search: {}", search_params.location);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {e}. Try broadening your search criteria (remove date/price filters) or check the location spelling."
            ))])),
        }
    }

    /// Get detailed information about a specific Airbnb listing including
    /// description, amenities, house rules, and photos.
    #[tool(
        name = "airbnb_listing_details",
        description = "Get detailed information about a specific Airbnb listing including description, amenities, house rules, photos, and host info. Requires a listing ID from airbnb_search.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_listing_details(
        &self,
        Parameters(params): Parameters<DetailToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.get_detail_with_price(&params.id).await {
            Ok(detail) => {
                let text = detail.to_string();
                let uri = format!("airbnb://listing/{}", params.id);
                let name = format!("Listing: {}", detail.name);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get listing details for ID '{}': {e}. Verify the listing ID is correct — use airbnb_search to find valid IDs.",
                params.id
            ))])),
        }
    }

    /// Get reviews for an Airbnb listing with ratings summary and pagination.
    #[tool(
        name = "airbnb_reviews",
        description = "Get reviews for an Airbnb listing including ratings summary, individual reviews with comments, and pagination support. Requires a listing ID. Use cursor from previous response to load more reviews.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_reviews(
        &self,
        Parameters(params): Parameters<ReviewsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .client
            .get_reviews(&params.id, params.cursor.as_deref())
            .await
        {
            Ok(page) => {
                let text = page.to_string();
                let uri = format!("airbnb://listing/{}/reviews", params.id);
                let name = format!("Reviews: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get reviews for listing '{}': {e}. The listing may have no reviews yet.",
                params.id
            ))])),
        }
    }

    /// Get price and availability calendar for an Airbnb listing.
    #[tool(
        name = "airbnb_price_calendar",
        description = "Get price and availability calendar for an Airbnb listing showing daily prices, availability status, and minimum night requirements. Useful for analyzing seasonal pricing and finding available dates.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_price_calendar(
        &self,
        Parameters(params): Parameters<CalendarToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(3).clamp(1, 12);

        match self.client.get_price_calendar(&params.id, months).await {
            Ok(calendar) => {
                let text = calendar.to_string();
                let uri = format!("airbnb://listing/{}/calendar", params.id);
                let name = format!("Calendar: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get price calendar for listing '{}': {e}. The listing may be unlisted or the calendar unavailable.",
                params.id
            ))])),
        }
    }

    /// Get the host profile for an Airbnb listing.
    #[tool(
        name = "airbnb_host_profile",
        description = "Get detailed host profile including superhost status, response rate, languages, bio, and listing count. Requires a listing ID to identify the host.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_host_profile(
        &self,
        Parameters(params): Parameters<HostProfileToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.get_host_profile(&params.id).await {
            Ok(profile) => {
                let text = profile.to_string();
                let uri = format!("airbnb://listing/{}/host", params.id);
                let name = format!("Host: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get host profile for listing '{}': {e}. Try airbnb_listing_details instead for basic host info.",
                params.id
            ))])),
        }
    }

    /// Get aggregated neighborhood statistics from Airbnb listings.
    #[tool(
        name = "airbnb_neighborhood_stats",
        description = "Get aggregated statistics for a neighborhood: average/median prices, ratings, property type distribution, and superhost percentage. Use this for market analysis and price benchmarking — does not require a listing ID, only a location.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_neighborhood_stats(
        &self,
        Parameters(params): Parameters<NeighborhoodStatsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let location = params.location;
        let search_params = SearchParams {
            location: location.clone(),
            checkin: params.checkin,
            checkout: params.checkout,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: params.property_type,
            cursor: None,
        };

        match self.client.get_neighborhood_stats(&search_params).await {
            Ok(stats) => {
                let text = stats.to_string();
                let uri = format!("airbnb://neighborhood/{location}");
                let name = format!("Neighborhood: {location}");
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get neighborhood stats for '{location}': {e}. Try a broader location name or check spelling."
            ))])),
        }
    }

    /// Get occupancy estimate for an Airbnb listing.
    #[tool(
        name = "airbnb_occupancy_estimate",
        description = "Estimate occupancy rate, average prices (weekday vs weekend), and monthly breakdown for a listing based on calendar data. Useful for hosts evaluating rental income potential.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_occupancy_estimate(
        &self,
        Parameters(params): Parameters<OccupancyEstimateToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(3).clamp(1, 12);

        match self.client.get_occupancy_estimate(&params.id, months).await {
            Ok(estimate) => {
                let text = estimate.to_string();
                let uri = format!("airbnb://listing/{}/occupancy", params.id);
                let name = format!("Occupancy: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get occupancy estimate for listing '{}': {e}. This requires calendar data — verify the listing ID.",
                params.id
            ))])),
        }
    }

    // ---- Analytical tools ----

    /// Compare multiple Airbnb listings side-by-side or analyze an entire market.
    #[tool(
        name = "airbnb_compare_listings",
        description = "Compare 2-100+ Airbnb listings side-by-side with price percentiles, ratings, and market summary. Provide listing IDs for detailed comparison (2-10), OR a location for market-scale comparison (up to 100 listings via paginated search). Returns ranking table with percentile positions.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    #[allow(clippy::too_many_lines)]
    async fn airbnb_compare_listings(
        &self,
        Parameters(params): Parameters<CompareListingsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.ids.is_none() && params.location.is_none() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Provide either `ids` (list of listing IDs) or `location` for market-scale comparison.",
            )]));
        }

        let mut pages_fetched: u32 = 0;
        let listings = if let Some(ref ids) = params.ids {
            // Mode 1: Fetch by IDs — use search results for lightweight comparison
            let mut all = Vec::new();
            for id in ids.iter().take(10) {
                match self.get_detail_with_price(id).await {
                    Ok(d) => all.push(crate::domain::listing::Listing {
                        id: d.id,
                        name: d.name,
                        location: d.location,
                        price_per_night: d.price_per_night,
                        currency: d.currency,
                        rating: d.rating,
                        review_count: d.review_count,
                        thumbnail_url: None,
                        property_type: d.property_type,
                        host_name: d.host_name,
                        host_id: d.host_id,
                        url: d.url,
                        is_superhost: d.host_is_superhost,
                        is_guest_favorite: None,
                        instant_book: d.instant_book,
                        total_price: None,
                        photos: d.photos,
                        latitude: d.latitude,
                        longitude: d.longitude,
                    }),
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Failed to fetch listing '{id}': {e}"
                        ))]));
                    }
                }
            }
            all
        } else {
            // Mode 2: Location discovery — paginate search
            let location = params.location.as_deref().unwrap_or("");
            let max = params.max_listings.unwrap_or(20).clamp(2, 100) as usize;
            let max_pages = max.div_ceil(20);
            let mut all = Vec::new();
            let mut cursor = None;

            for _ in 0..max_pages {
                let sp = SearchParams {
                    location: location.to_string(),
                    checkin: params.checkin.clone(),
                    checkout: params.checkout.clone(),
                    adults: None,
                    children: None,
                    infants: None,
                    pets: None,
                    min_price: None,
                    max_price: None,
                    property_type: params.property_type.clone(),
                    cursor: cursor.clone(),
                };
                match self.client.search_listings(&sp).await {
                    Ok(result) => {
                        pages_fetched += 1;
                        all.extend(result.listings);
                        cursor = result.next_cursor;
                        if cursor.is_none() || all.len() >= max {
                            break;
                        }
                    }
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Search failed for '{location}': {e}"
                        ))]));
                    }
                }
            }
            all.truncate(max);
            all
        };

        if listings.len() < 2 {
            return Ok(CallToolResult::error(vec![Content::text(
                "Need at least 2 listings to compare. Try a different location or provide more IDs.",
            )]));
        }

        let result = analytics::compute_compare_listings(&listings, None);
        let key = params
            .ids
            .as_ref()
            .map(|ids| ids.join("_"))
            .or(params.location.clone())
            .unwrap_or_default();
        let text = result.to_string();
        let uri = format!("airbnb://analysis/compare/{key}");
        let name = format!("Comparison: {key}");
        self.resources.insert(uri, name, text.clone()).await;

        // Prepend pagination metadata for location-discovery mode
        let output = if params.ids.is_none() {
            format!(
                "Fetched {} listings across {} page(s).\n\n{text}",
                listings.len(),
                pages_fetched
            )
        } else {
            text
        };
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Analyze seasonal price trends for a listing.
    #[tool(
        name = "airbnb_price_trends",
        description = "Analyze seasonal price trends for an Airbnb listing: monthly averages, weekend vs weekday premiums, price volatility, peak/off-peak months, and day-of-week breakdown. Based on calendar data.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_price_trends(
        &self,
        Parameters(params): Parameters<PriceTrendsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(12).clamp(1, 12);

        match self.client.get_price_calendar(&params.id, months).await {
            Ok(calendar) => {
                let trends = analytics::compute_price_trends(&params.id, &calendar);
                let text = trends.to_string();
                let uri = format!("airbnb://analysis/price-trends/{}", params.id);
                let name = format!("Price Trends: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get price data for listing '{}': {e}",
                params.id
            ))])),
        }
    }

    /// Detect booking gaps and orphan nights in a listing's calendar.
    #[tool(
        name = "airbnb_gap_finder",
        description = "Detect booking gaps and orphan nights (1-3 night gaps between reservations) in an Airbnb listing's calendar. Shows potential lost revenue and suggests minimum stay adjustments. Essential for occupancy optimization.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_gap_finder(
        &self,
        Parameters(params): Parameters<GapFinderToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(3).clamp(1, 12);

        match self.client.get_price_calendar(&params.id, months).await {
            Ok(calendar) => {
                let result = analytics::compute_gap_finder(&params.id, &calendar);
                let text = result.to_string();
                let uri = format!("airbnb://analysis/gaps/{}", params.id);
                let name = format!("Gap Finder: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get calendar for listing '{}': {e}",
                params.id
            ))])),
        }
    }

    /// Estimate revenue potential for a listing or location.
    #[tool(
        name = "airbnb_revenue_estimate",
        description = "Estimate projected revenue for an Airbnb listing: ADR (Average Daily Rate), occupancy rate, monthly and annual revenue projections, and comparison vs neighborhood average. Provide a listing ID for specific estimates, or just a location for market-based projections.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_revenue_estimate(
        &self,
        Parameters(params): Parameters<RevenueEstimateToolParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.id.is_none() && params.location.is_none() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Provide either `id` (listing ID) or `location` for revenue estimation.",
            )]));
        }

        let months = params.months.unwrap_or(12).clamp(1, 12);
        let mut calendar = None;
        let mut neighborhood = None;
        let mut occupancy = None;
        let mut location = params.location.clone().unwrap_or_default();

        if let Some(ref id) = params.id {
            if let Ok(cal) = self.client.get_price_calendar(id, months).await {
                calendar = Some(cal);
            }
            if let Ok(occ) = self.client.get_occupancy_estimate(id, months).await {
                occupancy = Some(occ);
            }
            // Get location from detail if not provided
            if location.is_empty()
                && let Ok(detail) = self.get_detail_with_price(id).await
            {
                location.clone_from(&detail.location);
            }
        }

        if !location.is_empty() {
            let sp = SearchParams {
                location: location.clone(),
                ..SearchParams::default()
            };
            if let Ok(stats) = self.client.get_neighborhood_stats(&sp).await {
                neighborhood = Some(stats);
            }
        }

        let result = analytics::compute_revenue_estimate(
            params.id.as_deref(),
            &location,
            calendar.as_ref(),
            neighborhood.as_ref(),
            occupancy.as_ref(),
        );
        let text = result.to_string();
        let key = params.id.as_deref().unwrap_or(&location);
        let uri = format!("airbnb://analysis/revenue/{key}");
        let name = format!("Revenue Estimate: {key}");
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Score a listing's quality and optimization level.
    #[tool(
        name = "airbnb_listing_score",
        description = "Score an Airbnb listing's quality (0-100) across 6 categories: photos, description, amenities, reviews, host profile, and pricing vs market. Provides actionable improvement suggestions. Like a free listing audit.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_listing_score(
        &self,
        Parameters(params): Parameters<ListingScoreToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.get_detail_with_price(&params.id).await {
            Ok(detail) => {
                // Try to get neighborhood stats for pricing comparison
                let sp = SearchParams {
                    location: detail.location.clone(),
                    ..SearchParams::default()
                };
                let neighborhood = self.client.get_neighborhood_stats(&sp).await.ok();
                let score = analytics::compute_listing_score(&detail, neighborhood.as_ref());
                let text = score.to_string();
                let uri = format!("airbnb://analysis/score/{}", params.id);
                let name = format!("Listing Score: listing {}", params.id);
                self.resources.insert(uri, name, text.clone()).await;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get listing '{}': {e}",
                params.id
            ))])),
        }
    }

    /// Analyze a listing's amenities vs neighborhood competition.
    #[tool(
        name = "airbnb_amenity_analysis",
        description = "Compare an Airbnb listing's amenities against neighborhood competition. Identifies missing popular amenities and highlights unique ones you have. Helps optimize your listing to match or beat competitors.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_amenity_analysis(
        &self,
        Parameters(params): Parameters<AmenityAnalysisToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let detail = match self.get_detail_with_price(&params.id).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get listing '{}': {e}",
                    params.id
                ))]));
            }
        };

        let location = params.location.unwrap_or_else(|| detail.location.clone());

        // Fetch a sample of neighbor listings for amenity comparison
        let sp = SearchParams {
            location,
            ..SearchParams::default()
        };
        let neighbor_ids: Vec<String> = match self.client.search_listings(&sp).await {
            Ok(result) => result
                .listings
                .into_iter()
                .filter(|l| l.id != params.id)
                .take(5)
                .map(|l| l.id)
                .collect(),
            Err(_) => vec![],
        };

        let mut neighbor_details = Vec::new();
        for id in &neighbor_ids {
            if let Ok(d) = self.get_detail_with_price(id).await {
                neighbor_details.push(d);
            }
        }

        let analysis = analytics::compute_amenity_analysis(&detail, &neighbor_details);
        let text = analysis.to_string();
        let uri = format!("airbnb://analysis/amenities/{}", params.id);
        let name = format!("Amenity Analysis: listing {}", params.id);
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Compare multiple locations/neighborhoods side-by-side.
    #[tool(
        name = "airbnb_market_comparison",
        description = "Compare 2-5 Airbnb markets side-by-side: average/median prices, ratings, superhost percentage, and dominant property types. Ideal for deciding where to invest or list a property.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_market_comparison(
        &self,
        Parameters(params): Parameters<MarketComparisonToolParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.locations.len() < 2 {
            return Ok(CallToolResult::error(vec![Content::text(
                "Provide at least 2 locations to compare.",
            )]));
        }

        let mut stats = Vec::new();
        for location in params.locations.iter().take(5) {
            let sp = SearchParams {
                location: location.clone(),
                checkin: params.checkin.clone(),
                checkout: params.checkout.clone(),
                property_type: params.property_type.clone(),
                ..SearchParams::default()
            };
            match self.client.get_neighborhood_stats(&sp).await {
                Ok(s) => stats.push(s),
                Err(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to get stats for '{location}': {e}"
                    ))]));
                }
            }
        }

        let result = analytics::compute_market_comparison(&stats);
        let text = result.to_string();
        let key = params.locations.join("_");
        let uri = format!("airbnb://analysis/market/{key}");
        let name = format!("Market Comparison: {key}");
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Analyze a host's full property portfolio.
    #[tool(
        name = "airbnb_host_portfolio",
        description = "Analyze an Airbnb host's full portfolio: all their properties, average rating, pricing strategy, total reviews, and geographic distribution. Useful for competitive intelligence on professional operators. Requires any listing ID from the host.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_host_portfolio(
        &self,
        Parameters(params): Parameters<HostPortfolioToolParams>,
    ) -> Result<CallToolResult, McpError> {
        // Get host info from the listing
        let detail = match self.get_detail_with_price(&params.id).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get listing '{}': {e}",
                    params.id
                ))]));
            }
        };

        let host_name = detail
            .host_name
            .clone()
            .unwrap_or_else(|| "Unknown Host".to_string());
        let host_id = detail.host_id.clone();
        let is_superhost = detail.host_is_superhost;

        // Search for other listings by this host in the same location
        let sp = SearchParams {
            location: detail.location.clone(),
            ..SearchParams::default()
        };
        let host_listings: Vec<_> = match self.client.search_listings(&sp).await {
            Ok(result) => {
                let all_listings = result.listings;
                // Prefer filtering by host_id (more reliable than name matching)
                if let Some(ref hid) = host_id {
                    let by_id: Vec<_> = all_listings
                        .iter()
                        .filter(|l| l.host_id.as_deref() == Some(hid.as_str()))
                        .cloned()
                        .collect();
                    if by_id.is_empty() {
                        // Fall back to host_name matching if no host_id matches found
                        all_listings
                            .into_iter()
                            .filter(|l| l.host_name.as_deref() == detail.host_name.as_deref())
                            .collect()
                    } else {
                        by_id
                    }
                } else {
                    // No host_id available, use host_name matching
                    all_listings
                        .into_iter()
                        .filter(|l| l.host_name.as_deref() == detail.host_name.as_deref())
                        .collect()
                }
            }
            Err(_) => vec![],
        };

        // If no other listings found via search, create one from the detail we have
        let listings = if host_listings.is_empty() {
            vec![crate::domain::listing::Listing {
                id: detail.id.clone(),
                name: detail.name.clone(),
                location: detail.location.clone(),
                price_per_night: detail.price_per_night,
                currency: detail.currency.clone(),
                rating: detail.rating,
                review_count: detail.review_count,
                thumbnail_url: None,
                property_type: detail.property_type.clone(),
                host_name: detail.host_name.clone(),
                host_id: detail.host_id.clone(),
                url: detail.url.clone(),
                is_superhost: detail.host_is_superhost,
                is_guest_favorite: None,
                instant_book: detail.instant_book,
                total_price: None,
                photos: detail.photos.clone(),
                latitude: detail.latitude,
                longitude: detail.longitude,
            }]
        } else {
            host_listings
        };

        let result = analytics::compute_host_portfolio(
            &host_name,
            host_id.as_deref(),
            is_superhost,
            &listings,
        );
        let text = result.to_string();
        let uri = format!("airbnb://analysis/portfolio/{}", params.id);
        let name = format!("Host Portfolio: listing {}", params.id);
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Analyze review sentiment for a listing.
    #[tool(
        name = "airbnb_review_sentiment",
        description = "Analyze guest review sentiment for an Airbnb listing: positive/negative/neutral breakdown, recurring themes (cleanliness, location, communication, amenities, value), and top keywords. Helps identify strengths and weaknesses from guest feedback.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_review_sentiment(
        &self,
        Parameters(params): Parameters<ReviewSentimentToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_pages = params.max_pages.unwrap_or(5).clamp(1, 20);
        let mut all_reviews = Vec::new();
        let mut cursor = None;

        for _ in 0..max_pages {
            match self.client.get_reviews(&params.id, cursor.as_deref()).await {
                Ok(page) => {
                    all_reviews.extend(page.reviews);
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    if all_reviews.is_empty() {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Failed to get reviews for listing '{}': {e}",
                            params.id
                        ))]));
                    }
                    break;
                }
            }
        }

        let result = analytics::compute_review_sentiment(&params.id, &all_reviews);
        let text = result.to_string();
        let uri = format!("airbnb://analysis/sentiment/{}", params.id);
        let name = format!("Review Sentiment: listing {}", params.id);
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Analyze a listing's competitive positioning vs its market.
    #[tool(
        name = "airbnb_competitive_positioning",
        description = "Evaluate an Airbnb listing's competitive position across 5 axes: price value, rating, amenity count, review volume, and occupancy. Returns percentile rankings, overall competitiveness score (0-100), strengths, and weaknesses vs the neighborhood.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_competitive_positioning(
        &self,
        Parameters(params): Parameters<CompetitivePositioningToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let detail = match self.get_detail_with_price(&params.id).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get listing '{}': {e}",
                    params.id
                ))]));
            }
        };

        let location = params.location.unwrap_or_else(|| detail.location.clone());
        let sp = SearchParams {
            location: location.clone(),
            ..SearchParams::default()
        };

        let neighborhood = match self.client.get_neighborhood_stats(&sp).await {
            Ok(n) => n,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get neighborhood stats for '{location}': {e}",
                ))]));
            }
        };
        let occupancy = self.client.get_occupancy_estimate(&params.id, 3).await.ok();

        // Get amenity analysis for the amenity axis
        let amenity_analysis = if let Ok(search_result) = self.client.search_listings(&sp).await {
            let neighbor_ids: Vec<String> = search_result
                .listings
                .into_iter()
                .filter(|l| l.id != params.id)
                .take(5)
                .map(|l| l.id)
                .collect();
            let mut neighbor_details = Vec::new();
            for id in &neighbor_ids {
                if let Ok(d) = self.get_detail_with_price(id).await {
                    neighbor_details.push(d);
                }
            }
            Some(analytics::compute_amenity_analysis(
                &detail,
                &neighbor_details,
            ))
        } else {
            None
        };

        let result = analytics::compute_competitive_positioning(
            &detail,
            &neighborhood,
            occupancy.as_ref(),
            amenity_analysis.as_ref(),
        );
        let text = result.to_string();
        let uri = format!("airbnb://analysis/positioning/{}", params.id);
        let name = format!("Competitive Positioning: listing {}", params.id);
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Suggest optimal pricing for a listing based on market data.
    #[tool(
        name = "airbnb_optimal_pricing",
        description = "Suggest optimal pricing for an Airbnb listing based on neighborhood comparables, seasonal trends, rating premium, and amenity analysis. Returns recommended price, range, weekday/weekend split, and detailed reasoning.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_optimal_pricing(
        &self,
        Parameters(params): Parameters<OptimalPricingToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let detail = match self.get_detail_with_price(&params.id).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get listing '{}': {e}",
                    params.id
                ))]));
            }
        };

        let location = params.location.unwrap_or_else(|| detail.location.clone());
        let sp = SearchParams {
            location,
            ..SearchParams::default()
        };

        let neighborhood = self.client.get_neighborhood_stats(&sp).await.ok();
        let price_trends = match self.client.get_price_calendar(&params.id, 12).await {
            Ok(calendar) => Some(analytics::compute_price_trends(&params.id, &calendar)),
            Err(_) => None,
        };

        // Get amenity analysis
        let amenity_analysis = if let Ok(search_result) = self.client.search_listings(&sp).await {
            let neighbor_ids: Vec<String> = search_result
                .listings
                .into_iter()
                .filter(|l| l.id != params.id)
                .take(5)
                .map(|l| l.id)
                .collect();
            let mut neighbor_details = Vec::new();
            for id in &neighbor_ids {
                if let Ok(d) = self.get_detail_with_price(id).await {
                    neighbor_details.push(d);
                }
            }
            Some(analytics::compute_amenity_analysis(
                &detail,
                &neighbor_details,
            ))
        } else {
            None
        };

        let result = analytics::compute_optimal_pricing(
            &detail,
            neighborhood.as_ref(),
            price_trends.as_ref(),
            amenity_analysis.as_ref(),
        );
        let text = result.to_string();
        let uri = format!("airbnb://analysis/pricing/{}", params.id);
        let name = format!("Optimal Pricing: listing {}", params.id);
        self.resources.insert(uri, name, text.clone()).await;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[tool_handler]
impl ServerHandler for AirbnbMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Airbnb MCP server for searching and analyzing short-term rental listings.\n\
                 \n\
                 ## Data Tools\n\
                 Start with airbnb_search to find listings by location. Each result includes a listing ID \
                 you can use with other tools:\n\
                 - airbnb_listing_details: full description, amenities, house rules, photos, capacity\n\
                 - airbnb_reviews: guest ratings and comments (paginated via cursor)\n\
                 - airbnb_price_calendar: daily prices and availability for 1-12 months\n\
                 - airbnb_host_profile: host bio, superhost status, response rate, languages\n\
                 - airbnb_occupancy_estimate: occupancy rate, weekday vs weekend pricing, monthly breakdown\n\
                 - airbnb_neighborhood_stats: area-level avg/median prices, ratings, property types\n\
                 \n\
                 ## Analytical Tools\n\
                 - airbnb_compare_listings: compare 2-100+ listings side-by-side with percentile rankings\n\
                 - airbnb_price_trends: seasonal pricing analysis (peak/off-peak, weekend premium, volatility)\n\
                 - airbnb_gap_finder: detect orphan nights and booking gaps with lost revenue estimate\n\
                 - airbnb_revenue_estimate: project ADR, occupancy, monthly/annual revenue\n\
                 - airbnb_listing_score: quality audit (0-100) with improvement suggestions\n\
                 - airbnb_amenity_analysis: missing popular amenities vs neighborhood competition\n\
                 - airbnb_market_comparison: compare 2-5 neighborhoods side-by-side\n\
                 - airbnb_host_portfolio: analyze a host's full property portfolio\n\
                 - airbnb_review_sentiment: keyword-based sentiment analysis of guest reviews\n\
                 - airbnb_competitive_positioning: 5-axis competitive score vs neighborhood\n\
                 - airbnb_optimal_pricing: data-driven pricing recommendation with reasoning\n\
                 \n\
                 ## Resources\n\
                 Data fetched by tools is cached as MCP resources. Use resource URIs to reference \
                 previously fetched data without re-scraping.\n\
                 \n\
                 ## Tips\n\
                 - Use airbnb_compare_listings with a location to analyze an entire market (up to 100 listings).\n\
                 - Use airbnb_listing_score + airbnb_amenity_analysis for a complete listing audit.\n\
                 - Use airbnb_revenue_estimate to evaluate investment potential.\n\
                 - Pagination: pass the cursor from a previous response to get the next page."
                    .into(),
            ),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let entries = self.resources.list().await;
        let resources: Vec<Resource> = entries
            .into_iter()
            .map(|(uri, name)| Resource {
                annotations: None,
                raw: RawResource {
                    uri,
                    name,
                    title: None,
                    description: None,
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
            })
            .collect();
        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let templates = vec![
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://listing/{id}".into(),
                    name: "Airbnb Listing".into(),
                    title: Some("Listing details".into()),
                    description: Some(
                        "Full listing details (fetched via airbnb_listing_details)".into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://listing/{id}/calendar".into(),
                    name: "Price Calendar".into(),
                    title: Some("Price & availability calendar".into()),
                    description: Some(
                        "Daily prices and availability (fetched via airbnb_price_calendar)".into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://listing/{id}/reviews".into(),
                    name: "Reviews".into(),
                    title: Some("Guest reviews".into()),
                    description: Some(
                        "Guest reviews and ratings (fetched via airbnb_reviews)".into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://listing/{id}/host".into(),
                    name: "Host Profile".into(),
                    title: Some("Host profile".into()),
                    description: Some(
                        "Host bio, superhost status, response rate (fetched via airbnb_host_profile)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://listing/{id}/occupancy".into(),
                    name: "Occupancy Estimate".into(),
                    title: Some("Occupancy estimate".into()),
                    description: Some(
                        "Occupancy rate and revenue breakdown (fetched via airbnb_occupancy_estimate)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://search/{location}".into(),
                    name: "Search Results".into(),
                    title: Some("Search results".into()),
                    description: Some(
                        "Listings found for a location (fetched via airbnb_search)".into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://neighborhood/{location}".into(),
                    name: "Neighborhood Stats".into(),
                    title: Some("Neighborhood statistics".into()),
                    description: Some(
                        "Area-level price/rating stats (fetched via airbnb_neighborhood_stats)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
            ResourceTemplate {
                annotations: None,
                raw: RawResourceTemplate {
                    uri_template: "airbnb://analysis/{type}/{id}".into(),
                    name: "Analysis Result".into(),
                    title: Some("Analytical tool result".into()),
                    description: Some(
                        "Cached result from analytical tools (compare, trends, gaps, revenue, score, amenities, market, portfolio)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    icons: None,
                },
            },
        ];
        Ok(ListResourceTemplatesResult {
            resource_templates: templates,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match self.resources.get(&request.uri).await {
            Some(entry) => Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(entry.text, request.uri)],
            }),
            None => Err(McpError::resource_not_found(
                format!("resource not found: {}", request.uri),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::calendar::{CalendarDay, PriceCalendar};
    use crate::error::AirbnbError;
    use crate::test_helpers::*;

    fn extract_text(result: &CallToolResult) -> &str {
        result.content[0]
            .raw
            .as_text()
            .expect("expected text content")
            .text
            .as_str()
    }

    fn make_server(mock: MockAirbnbClient) -> AirbnbMcpServer {
        AirbnbMcpServer::new(Arc::new(mock))
    }

    #[tokio::test]
    async fn search_returns_formatted_listings() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![
                make_listing("1", "Cozy Flat", 100.0),
                make_listing("2", "Beach House", 250.0),
            ]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Paris".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Cozy Flat"));
        assert!(text.contains("Beach House"));
        assert!(text.contains("ID: 1"));
        assert!(text.contains("ID: 2"));
        assert!(text.contains("$100"));
        assert!(text.contains("$250"));
        assert!(text.contains("Found 2 listings"));
    }

    #[tokio::test]
    async fn search_empty_results() {
        let mock = MockAirbnbClient::new().with_search(|_| Ok(make_search_result(vec![])));
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Nowhere".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("No listings found"));
    }

    #[tokio::test]
    async fn search_with_pagination_cursor() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            let mut result = make_search_result(vec![make_listing("1", "Place", 50.0)]);
            result.next_cursor = Some("abc123".to_string());
            Ok(result)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Tokyo".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("abc123"));
        assert!(text.contains("More results available"));
    }

    #[tokio::test]
    async fn search_error_returns_error_result() {
        let mock = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Paris".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Search failed"));
    }

    #[tokio::test]
    async fn listing_details_success() {
        let mock = MockAirbnbClient::new().with_detail(|id| {
            let mut detail = make_listing_detail(id);
            detail.name = "Luxurious Villa".to_string();
            Ok(detail)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_listing_details(Parameters(DetailToolParams { id: "42".into() }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Luxurious Villa"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn listing_details_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_listing_details(Parameters(DetailToolParams { id: "999".into() }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing details"));
    }

    #[tokio::test]
    async fn reviews_success() {
        let mock = MockAirbnbClient::new().with_reviews(|id, _| {
            let reviews = vec![
                make_review("Alice", "Amazing place!"),
                make_review("Bob", "Very clean."),
            ];
            let mut page = make_reviews_page(id, reviews);
            page.summary = Some(make_reviews_summary());
            Ok(page)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_reviews(Parameters(ReviewsToolParams {
                id: "42".into(),
                cursor: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Alice"));
        assert!(text.contains("Amazing place!"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn reviews_error() {
        let mock = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "no reviews data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_reviews(Parameters(ReviewsToolParams {
                id: "42".into(),
                cursor: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get reviews"));
    }

    #[tokio::test]
    async fn calendar_success() {
        let mock = MockAirbnbClient::new().with_calendar(|id, _months| {
            let days = vec![
                make_calendar_day("2025-06-01", Some(120.0), true),
                make_calendar_day("2025-06-02", Some(130.0), false),
            ];
            Ok(make_price_calendar(id, days))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_price_calendar(Parameters(CalendarToolParams {
                id: "42".into(),
                months: Some(3),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("2025-06-01"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn calendar_error() {
        let mock = MockAirbnbClient::new().with_calendar(|_, _| {
            Err(AirbnbError::Parse {
                reason: "no calendar data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_price_calendar(Parameters(CalendarToolParams {
                id: "42".into(),
                months: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get price calendar"));
    }

    #[tokio::test]
    async fn calendar_months_clamped() {
        // Verify months parameter is clamped to [1, 12]
        let mock = MockAirbnbClient::new().with_calendar(|id, months| {
            // Encode months in the currency field to verify clamping
            Ok(PriceCalendar {
                listing_id: id.to_string(),
                currency: format!("months={months}"),
                days: vec![CalendarDay {
                    date: "2025-01-01".into(),
                    price: Some(100.0),
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                }],
                average_price: None,
                occupancy_rate: None,
                min_price: None,
                max_price: None,
            })
        });
        let server = make_server(mock);

        // months=0 should be clamped to 1
        let result = server
            .airbnb_price_calendar(Parameters(CalendarToolParams {
                id: "1".into(),
                months: Some(0),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("months=1"), "Expected months=1, got: {text}");

        // months=15 should be clamped to 12
        let result = server
            .airbnb_price_calendar(Parameters(CalendarToolParams {
                id: "1".into(),
                months: Some(15),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(
            text.contains("months=12"),
            "Expected months=12, got: {text}"
        );
    }

    #[tokio::test]
    async fn host_profile_success() {
        let mock = MockAirbnbClient::new().with_host_profile(|_| {
            let mut profile = make_host_profile("Super Alice");
            profile.is_superhost = Some(true);
            Ok(profile)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_host_profile(Parameters(HostProfileToolParams { id: "42".into() }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Super Alice"));
        assert!(text.contains("Superhost"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn host_profile_error() {
        let mock = MockAirbnbClient::new().with_host_profile(|_| {
            Err(AirbnbError::Parse {
                reason: "no host data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_host_profile(Parameters(HostProfileToolParams { id: "42".into() }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get host profile"));
    }

    #[tokio::test]
    async fn neighborhood_stats_success() {
        let mock = MockAirbnbClient::new().with_neighborhood(|params| {
            use crate::domain::analytics::NeighborhoodStats;
            Ok(NeighborhoodStats {
                location: params.location.clone(),
                total_listings: 15,
                average_price: Some(120.0),
                median_price: Some(110.0),
                price_range: Some((50.0, 300.0)),
                average_rating: Some(4.6),
                property_type_distribution: vec![],
                superhost_percentage: Some(40.0),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_neighborhood_stats(Parameters(NeighborhoodStatsToolParams {
                location: "Paris".into(),
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Paris"));
        assert!(text.contains("15"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn neighborhood_stats_error() {
        let mock = MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited));
        let server = make_server(mock);
        let result = server
            .airbnb_neighborhood_stats(Parameters(NeighborhoodStatsToolParams {
                location: "Paris".into(),
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get neighborhood stats"));
    }

    #[tokio::test]
    async fn occupancy_estimate_success() {
        let mock = MockAirbnbClient::new().with_occupancy(|id, _| {
            use crate::domain::analytics::OccupancyEstimate;
            Ok(OccupancyEstimate {
                listing_id: id.to_string(),
                period_start: "2025-06-01".to_string(),
                period_end: "2025-08-31".to_string(),
                total_days: 92,
                occupied_days: 60,
                available_days: 32,
                occupancy_rate: 65.2,
                average_available_price: Some(150.0),
                weekend_avg_price: Some(180.0),
                weekday_avg_price: Some(130.0),
                monthly_breakdown: vec![],
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "42".into(),
                months: Some(3),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("listing 42"));
        assert!(text.contains("65.2%"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn occupancy_estimate_error() {
        let mock = MockAirbnbClient::new().with_occupancy(|_, _| {
            Err(AirbnbError::Parse {
                reason: "no calendar data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "42".into(),
                months: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get occupancy estimate"));
    }

    #[tokio::test]
    async fn search_forwards_all_params() {
        use std::sync::Arc as StdArc;
        use std::sync::Mutex as StdMutex;

        let captured = StdArc::new(StdMutex::new(None::<SearchParams>));
        let captured_clone = captured.clone();
        let mock = MockAirbnbClient::new().with_search(move |params| {
            *captured_clone.lock().unwrap() = Some(params.clone());
            Ok(make_search_result(vec![]))
        });
        let server = make_server(mock);
        let _ = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Paris".into(),
                checkin: Some("2025-07-01".into()),
                checkout: Some("2025-07-05".into()),
                adults: Some(2),
                children: Some(1),
                infants: Some(0),
                pets: Some(1),
                min_price: Some(50),
                max_price: Some(200),
                property_type: Some("Entire home".into()),
                cursor: Some("page2".into()),
            }))
            .await
            .unwrap();

        let params = captured.lock().unwrap().take().unwrap();
        assert_eq!(params.location, "Paris");
        assert_eq!(params.checkin, Some("2025-07-01".into()));
        assert_eq!(params.checkout, Some("2025-07-05".into()));
        assert_eq!(params.adults, Some(2));
        assert_eq!(params.children, Some(1));
        assert_eq!(params.infants, Some(0));
        assert_eq!(params.pets, Some(1));
        assert_eq!(params.min_price, Some(50));
        assert_eq!(params.max_price, Some(200));
        assert_eq!(params.property_type, Some("Entire home".into()));
        assert_eq!(params.cursor, Some("page2".into()));
    }

    #[tokio::test]
    async fn occupancy_months_clamped() {
        let mock = MockAirbnbClient::new().with_occupancy(|id, months| {
            use crate::domain::analytics::OccupancyEstimate;
            Ok(OccupancyEstimate {
                listing_id: id.to_string(),
                period_start: format!("months={months}"),
                period_end: String::new(),
                total_days: 0,
                occupied_days: 0,
                available_days: 0,
                occupancy_rate: 0.0,
                average_available_price: None,
                weekend_avg_price: None,
                weekday_avg_price: None,
                monthly_breakdown: vec![],
            })
        });
        let server = make_server(mock);

        // months=0 => clamped to 1
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "1".into(),
                months: Some(0),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("months=1"), "Expected months=1, got: {text}");

        // months=99 => clamped to 12
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "1".into(),
                months: Some(99),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(
            text.contains("months=12"),
            "Expected months=12, got: {text}"
        );
    }

    #[tokio::test]
    async fn search_with_rating_and_property_type() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            let mut listing = make_listing("1", "Test Place", 100.0);
            listing.rating = Some(4.92);
            listing.review_count = 42;
            listing.property_type = Some("Entire villa".into());
            Ok(make_search_result(vec![listing]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Bali".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("4.9"), "Should contain rating");
        assert!(text.contains("42 reviews"), "Should contain review count");
        assert!(
            text.contains("Entire villa"),
            "Should contain property type"
        );
    }

    #[tokio::test]
    async fn search_listing_without_rating() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            let mut listing = make_listing("1", "No Rating Place", 80.0);
            listing.rating = None;
            listing.property_type = None;
            Ok(make_search_result(vec![listing]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Tokyo".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("No Rating Place"));
        assert!(!text.contains("Rating:"), "Should not contain Rating line");
    }

    #[tokio::test]
    async fn listing_detail_output_contains_key_fields() {
        let mock = MockAirbnbClient::new().with_detail(|id| {
            let mut detail = make_listing_detail(id);
            detail.name = "Luxury Penthouse".into();
            detail.location = "Manhattan, NY".into();
            detail.price_per_night = 350.0;
            detail.amenities = vec!["Pool".into(), "Gym".into()];
            Ok(detail)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_listing_details(Parameters(DetailToolParams { id: "99".into() }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Luxury Penthouse"));
        assert!(text.contains("Manhattan, NY"));
        assert!(text.contains("350"));
        assert!(text.contains("Pool"));
        assert!(text.contains("Gym"));
    }

    #[tokio::test]
    async fn reviews_with_summary_and_cursor() {
        let mock = MockAirbnbClient::new().with_reviews(|id, _| {
            let mut page = make_reviews_page(id, vec![make_review("Eve", "Loved it!")]);
            page.summary = Some(make_reviews_summary());
            page.next_cursor = Some("next_page_token".into());
            Ok(page)
        });
        let server = make_server(mock);
        let result = server
            .airbnb_reviews(Parameters(ReviewsToolParams {
                id: "42".into(),
                cursor: None,
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Eve"));
        assert!(text.contains("Loved it!"));
        assert!(
            text.contains("4.7"),
            "Should contain overall rating from summary"
        );
        assert!(
            text.contains("More reviews available"),
            "Should contain pagination indicator"
        );
    }

    #[test]
    fn server_info_correct() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let info = server.get_info();
        assert!(info.instructions.is_some());
        let instructions = info.instructions.unwrap();
        assert!(instructions.contains("airbnb_search"));
        assert!(instructions.contains("airbnb_listing_details"));
        assert!(instructions.contains("airbnb_reviews"));
        assert!(instructions.contains("airbnb_price_calendar"));
        assert!(instructions.contains("airbnb_host_profile"));
        assert!(instructions.contains("airbnb_neighborhood_stats"));
        assert!(instructions.contains("airbnb_occupancy_estimate"));
        assert!(instructions.contains("airbnb_compare_listings"));
        assert!(instructions.contains("airbnb_price_trends"));
        assert!(instructions.contains("airbnb_gap_finder"));
        assert!(instructions.contains("airbnb_revenue_estimate"));
        assert!(instructions.contains("airbnb_listing_score"));
        assert!(instructions.contains("airbnb_amenity_analysis"));
        assert!(instructions.contains("airbnb_market_comparison"));
        assert!(instructions.contains("airbnb_host_portfolio"));
        // Verify capabilities include both tools and resources
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
    }

    // ---- Analytical tools tests ----

    #[tokio::test]
    async fn price_trends_success() {
        let mock = MockAirbnbClient::new().with_calendar(|id, _| {
            let days = vec![
                make_calendar_day("2025-06-06", Some(200.0), true),
                make_calendar_day("2025-06-07", Some(250.0), true),
                make_calendar_day("2025-06-09", Some(100.0), true),
            ];
            Ok(make_price_calendar(id, days))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_price_trends(Parameters(PriceTrendsToolParams {
                id: "42".into(),
                months: Some(6),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Price Trends: listing 42"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn price_trends_error() {
        let mock = MockAirbnbClient::new().with_calendar(|_, _| {
            Err(AirbnbError::Parse {
                reason: "fail".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_price_trends(Parameters(PriceTrendsToolParams {
                id: "42".into(),
                months: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn gap_finder_success() {
        let mock = MockAirbnbClient::new().with_calendar(|id, _| {
            let days = vec![
                make_calendar_day("2025-06-01", Some(100.0), false),
                make_calendar_day("2025-06-02", Some(150.0), true),
                make_calendar_day("2025-06-03", Some(100.0), false),
            ];
            Ok(make_price_calendar(id, days))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_gap_finder(Parameters(GapFinderToolParams {
                id: "42".into(),
                months: Some(3),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Gap Analysis: listing 42"));
        assert!(text.contains("orphan"));
    }

    #[tokio::test]
    async fn compare_listings_by_location_success() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![
                make_listing("1", "A", 100.0),
                make_listing("2", "B", 200.0),
                make_listing("3", "C", 150.0),
            ]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_compare_listings(Parameters(CompareListingsToolParams {
                ids: None,
                location: Some("Paris".into()),
                max_listings: Some(20),
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Listing Comparison (3 listings)"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn compare_listings_requires_ids_or_location() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let result = server
            .airbnb_compare_listings(Parameters(CompareListingsToolParams {
                ids: None,
                location: None,
                max_listings: None,
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn revenue_estimate_success() {
        let mock = MockAirbnbClient::new()
            .with_calendar(|id, _| {
                let days = vec![
                    make_calendar_day("2025-06-01", Some(100.0), true),
                    make_calendar_day("2025-06-02", Some(100.0), false),
                ];
                Ok(make_price_calendar(id, days))
            })
            .with_occupancy(|id, _| Ok(make_occupancy_estimate(id)));
        let server = make_server(mock);
        let result = server
            .airbnb_revenue_estimate(Parameters(RevenueEstimateToolParams {
                id: Some("42".into()),
                location: Some("Paris".into()),
                months: Some(12),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Revenue Estimate"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn revenue_estimate_requires_id_or_location() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let result = server
            .airbnb_revenue_estimate(Parameters(RevenueEstimateToolParams {
                id: None,
                location: None,
                months: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn listing_score_success() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let result = server
            .airbnb_listing_score(Parameters(ListingScoreToolParams { id: "42".into() }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Listing Score: 42"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn amenity_analysis_success() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "99", "Neighbor", 100.0,
            )]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_amenity_analysis(Parameters(AmenityAnalysisToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Amenity Analysis: listing 42"));
    }

    #[tokio::test]
    async fn market_comparison_success() {
        let mock = MockAirbnbClient::new().with_neighborhood(|params| {
            Ok(crate::domain::analytics::NeighborhoodStats {
                location: params.location.clone(),
                total_listings: 50,
                average_price: Some(120.0),
                median_price: Some(110.0),
                price_range: Some((50.0, 300.0)),
                average_rating: Some(4.5),
                property_type_distribution: vec![],
                superhost_percentage: Some(30.0),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_market_comparison(Parameters(MarketComparisonToolParams {
                locations: vec!["Paris".into(), "London".into()],
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Market Comparison"));
        assert!(text.contains("Paris"));
        assert!(text.contains("London"));
    }

    #[tokio::test]
    async fn market_comparison_requires_two_locations() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let result = server
            .airbnb_market_comparison(Parameters(MarketComparisonToolParams {
                locations: vec!["Paris".into()],
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn host_portfolio_success() {
        let mock = MockAirbnbClient::new().with_search(|_| {
            let mut l1 = make_listing("1", "Apt 1", 100.0);
            l1.host_name = Some("Test Host".into());
            let mut l2 = make_listing("2", "Apt 2", 200.0);
            l2.host_name = Some("Test Host".into());
            Ok(make_search_result(vec![l1, l2]))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_host_portfolio(Parameters(HostPortfolioToolParams { id: "42".into() }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(text.contains("Host Portfolio: Test Host"));
    }

    // ---- Resource store tests ----

    #[tokio::test]
    async fn resource_store_empty_initially() {
        let store = ResourceStore::default();
        assert!(store.list().await.is_empty());
    }

    #[tokio::test]
    async fn resource_store_insert_and_get() {
        let store = ResourceStore::default();
        store
            .insert("airbnb://listing/42", "Listing 42", "details".to_string())
            .await;

        let entry = store.get("airbnb://listing/42").await;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().text, "details");
    }

    #[tokio::test]
    async fn resource_store_list_populated() {
        let store = ResourceStore::default();
        store
            .insert("airbnb://listing/1", "Listing 1", "a".to_string())
            .await;
        store
            .insert("airbnb://listing/2", "Listing 2", "b".to_string())
            .await;

        let list = store.list().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn resource_store_get_missing_returns_none() {
        let store = ResourceStore::default();
        assert!(store.get("airbnb://nothing").await.is_none());
    }

    #[tokio::test]
    async fn resource_stored_after_listing_detail() {
        let mock = MockAirbnbClient::new().with_detail(|id| Ok(make_listing_detail(id)));
        let server = make_server(mock);

        // Fetch listing detail via tool
        let _ = server
            .airbnb_listing_details(Parameters(DetailToolParams { id: "42".into() }))
            .await
            .unwrap();

        // Resource should now be in the store
        let entry = server.resources.get("airbnb://listing/42").await;
        assert!(entry.is_some());
        assert!(entry.unwrap().name.contains("Listing"));
    }

    #[tokio::test]
    async fn resource_stored_after_search() {
        let mock = MockAirbnbClient::new()
            .with_search(|_| Ok(make_search_result(vec![make_listing("1", "Test", 100.0)])));
        let server = make_server(mock);

        let _ = server
            .airbnb_search(Parameters(SearchToolParams {
                location: "Paris".into(),
                checkin: None,
                checkout: None,
                adults: None,
                children: None,
                infants: None,
                pets: None,
                min_price: None,
                max_price: None,
                property_type: None,
                cursor: None,
            }))
            .await
            .unwrap();

        let entry = server.resources.get("airbnb://search/Paris").await;
        assert!(entry.is_some());
    }

    #[tokio::test]
    async fn resource_stored_after_calendar() {
        let mock = MockAirbnbClient::new().with_calendar(|id, _| {
            Ok(make_price_calendar(
                id,
                vec![make_calendar_day("2025-06-01", Some(100.0), true)],
            ))
        });
        let server = make_server(mock);

        let _ = server
            .airbnb_price_calendar(Parameters(CalendarToolParams {
                id: "42".into(),
                months: None,
            }))
            .await
            .unwrap();

        let entry = server.resources.get("airbnb://listing/42/calendar").await;
        assert!(entry.is_some());
    }

    #[test]
    fn server_capabilities_include_resources() {
        let mock = MockAirbnbClient::new();
        let server = make_server(mock);
        let info = server.get_info();
        assert!(info.capabilities.resources.is_some());
    }

    // ---- New analytical tool success tests ----

    #[tokio::test]
    async fn review_sentiment_success() {
        let mock = MockAirbnbClient::new().with_reviews(|id, _| {
            let reviews = vec![
                make_review("Alice", "Amazing place, super clean!"),
                make_review("Bob", "Terrible noise, very dirty."),
                make_review("Carol", "Great location, loved the view."),
            ];
            Ok(make_reviews_page(id, reviews))
        });
        let server = make_server(mock);
        let result = server
            .airbnb_review_sentiment(Parameters(ReviewSentimentToolParams {
                id: "42".into(),
                max_pages: Some(1),
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(
            text.contains("Review Sentiment"),
            "Should contain 'Review Sentiment', got: {text}"
        );
        assert!(text.contains("listing 42"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn competitive_positioning_success() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| {
                let mut detail = make_listing_detail(id);
                detail.amenities = vec!["WiFi".into(), "Pool".into(), "Kitchen".into()];
                Ok(detail)
            })
            .with_neighborhood(|params| {
                Ok(crate::domain::analytics::NeighborhoodStats {
                    location: params.location.clone(),
                    total_listings: 20,
                    average_price: Some(120.0),
                    median_price: Some(110.0),
                    price_range: Some((50.0, 300.0)),
                    average_rating: Some(4.5),
                    property_type_distribution: vec![],
                    superhost_percentage: Some(30.0),
                })
            })
            .with_search(|_| {
                Ok(make_search_result(vec![make_listing(
                    "99", "Neighbor", 110.0,
                )]))
            });
        let server = make_server(mock);
        let result = server
            .airbnb_competitive_positioning(Parameters(CompetitivePositioningToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(
            text.contains("Competitive Positioning"),
            "Should contain 'Competitive Positioning', got: {text}"
        );
        assert!(text.contains("listing 42"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    #[tokio::test]
    async fn optimal_pricing_success() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Ok(make_listing_detail(id)))
            .with_calendar(|id, _| {
                let days = vec![
                    make_calendar_day("2025-06-01", Some(100.0), true),
                    make_calendar_day("2025-06-02", Some(120.0), true),
                ];
                Ok(make_price_calendar(id, days))
            })
            .with_neighborhood(|params| {
                Ok(crate::domain::analytics::NeighborhoodStats {
                    location: params.location.clone(),
                    total_listings: 15,
                    average_price: Some(130.0),
                    median_price: Some(120.0),
                    price_range: Some((60.0, 250.0)),
                    average_rating: Some(4.4),
                    property_type_distribution: vec![],
                    superhost_percentage: Some(25.0),
                })
            })
            .with_search(|_| Ok(make_search_result(vec![])));
        let server = make_server(mock);
        let result = server
            .airbnb_optimal_pricing(Parameters(OptimalPricingToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        let text = extract_text(&result);
        assert!(
            text.contains("Pricing Recommendation"),
            "Should contain 'Pricing Recommendation', got: {text}"
        );
        assert!(text.contains("listing 42"));
        assert!(result.is_error.is_none() || result.is_error == Some(false));
    }

    // ---- Error propagation tests for analytical tools ----

    #[tokio::test]
    async fn gap_finder_error() {
        let mock = MockAirbnbClient::new().with_calendar(|_, _| {
            Err(AirbnbError::Parse {
                reason: "no calendar data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_gap_finder(Parameters(GapFinderToolParams {
                id: "42".into(),
                months: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get calendar"));
    }

    #[tokio::test]
    async fn listing_score_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_listing_score(Parameters(ListingScoreToolParams { id: "42".into() }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing"));
    }

    #[tokio::test]
    async fn amenity_analysis_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_amenity_analysis(Parameters(AmenityAnalysisToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing"));
    }

    #[tokio::test]
    async fn host_portfolio_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_host_portfolio(Parameters(HostPortfolioToolParams { id: "42".into() }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing"));
    }

    #[tokio::test]
    async fn market_comparison_error() {
        let mock = MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited));
        let server = make_server(mock);
        let result = server
            .airbnb_market_comparison(Parameters(MarketComparisonToolParams {
                locations: vec!["Paris".into(), "London".into()],
                checkin: None,
                checkout: None,
                property_type: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get stats"));
    }

    #[tokio::test]
    async fn review_sentiment_error() {
        let mock = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "no reviews data".into(),
            })
        });
        let server = make_server(mock);
        let result = server
            .airbnb_review_sentiment(Parameters(ReviewSentimentToolParams {
                id: "42".into(),
                max_pages: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get reviews"));
    }

    #[tokio::test]
    async fn competitive_positioning_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_competitive_positioning(Parameters(CompetitivePositioningToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing"));
    }

    #[tokio::test]
    async fn optimal_pricing_error() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let server = make_server(mock);
        let result = server
            .airbnb_optimal_pricing(Parameters(OptimalPricingToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = extract_text(&result);
        assert!(text.contains("Failed to get listing"));
    }

    // ---- Resource storage tests for new tools ----

    #[tokio::test]
    async fn resource_stored_after_review_sentiment() {
        let mock = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("Alice", "Great place!")],
            ))
        });
        let server = make_server(mock);

        let _ = server
            .airbnb_review_sentiment(Parameters(ReviewSentimentToolParams {
                id: "42".into(),
                max_pages: Some(1),
            }))
            .await
            .unwrap();

        let entry = server.resources.get("airbnb://analysis/sentiment/42").await;
        assert!(
            entry.is_some(),
            "Resource should be stored after review_sentiment"
        );
        assert!(entry.unwrap().name.contains("Review Sentiment"));
    }

    #[tokio::test]
    async fn resource_stored_after_competitive_positioning() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Ok(make_listing_detail(id)))
            .with_neighborhood(|params| Ok(make_neighborhood_stats(&params.location)))
            .with_search(|_| Ok(make_search_result(vec![])));
        let server = make_server(mock);

        let _ = server
            .airbnb_competitive_positioning(Parameters(CompetitivePositioningToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        let entry = server
            .resources
            .get("airbnb://analysis/positioning/42")
            .await;
        assert!(
            entry.is_some(),
            "Resource should be stored after competitive_positioning"
        );
        assert!(entry.unwrap().name.contains("Competitive Positioning"));
    }

    #[tokio::test]
    async fn resource_stored_after_optimal_pricing() {
        let mock = MockAirbnbClient::new()
            .with_detail(|id| Ok(make_listing_detail(id)))
            .with_calendar(|id, _| {
                Ok(make_price_calendar(
                    id,
                    vec![make_calendar_day("2025-06-01", Some(100.0), true)],
                ))
            })
            .with_search(|_| Ok(make_search_result(vec![])));
        let server = make_server(mock);

        let _ = server
            .airbnb_optimal_pricing(Parameters(OptimalPricingToolParams {
                id: "42".into(),
                location: None,
            }))
            .await
            .unwrap();

        let entry = server.resources.get("airbnb://analysis/pricing/42").await;
        assert!(
            entry.is_some(),
            "Resource should be stored after optimal_pricing"
        );
        assert!(entry.unwrap().name.contains("Optimal Pricing"));
    }
}
