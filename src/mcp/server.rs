use std::fmt::Write as _;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
};

use crate::domain::search_params::SearchParams;
use crate::ports::airbnb_client::AirbnbClient;

// ---------- Tool parameter types ----------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchToolParams {
    /// Location to search (e.g. "Paris, France", "Tokyo", "New York")
    pub location: String,
    /// Check-in date (YYYY-MM-DD format)
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD format)
    pub checkout: Option<String>,
    /// Number of adult guests
    pub adults: Option<u32>,
    /// Number of children
    pub children: Option<u32>,
    /// Number of infants
    pub infants: Option<u32>,
    /// Number of pets
    pub pets: Option<u32>,
    /// Minimum price per night (USD)
    pub min_price: Option<u32>,
    /// Maximum price per night (USD)
    pub max_price: Option<u32>,
    /// Property type filter (e.g. "Entire home", "Private room")
    pub property_type: Option<String>,
    /// Pagination cursor from previous search results
    pub cursor: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DetailToolParams {
    /// Airbnb listing ID (numeric string, e.g. "12345678")
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
    /// Location to analyze (e.g. "Paris, France", "Brooklyn, NY")
    pub location: String,
    /// Check-in date (YYYY-MM-DD format)
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD format)
    pub checkout: Option<String>,
    /// Property type filter (e.g. "Entire home", "Private room")
    pub property_type: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OccupancyEstimateToolParams {
    /// Airbnb listing ID
    pub id: String,
    /// Number of months to analyze (1-12, default: 3)
    pub months: Option<u32>,
}

// ---------- MCP Server ----------

#[derive(Clone)]
pub struct AirbnbMcpServer {
    client: Arc<dyn AirbnbClient>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AirbnbMcpServer {
    pub fn new(client: Arc<dyn AirbnbClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    /// Search Airbnb listings by location, dates, and guest count.
    /// Returns a list of available listings matching the search criteria.
    #[tool(
        name = "airbnb_search",
        description = "Search Airbnb listings by location, dates, and guest count. Returns a list of available listings with prices, ratings, and links.",
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
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {e}"
            ))])),
        }
    }

    /// Get detailed information about a specific Airbnb listing including
    /// description, amenities, house rules, and photos.
    #[tool(
        name = "airbnb_listing_details",
        description = "Get detailed information about a specific Airbnb listing including description, amenities, house rules, photos, and host info.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_listing_details(
        &self,
        Parameters(params): Parameters<DetailToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.get_listing_detail(&params.id).await {
            Ok(detail) => Ok(CallToolResult::success(vec![Content::text(
                detail.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get listing details: {e}"
            ))])),
        }
    }

    /// Get reviews for an Airbnb listing with ratings summary and pagination.
    #[tool(
        name = "airbnb_reviews",
        description = "Get reviews for an Airbnb listing including ratings summary, individual reviews with comments, and pagination support.",
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
            Ok(page) => Ok(CallToolResult::success(vec![Content::text(
                page.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get reviews: {e}"
            ))])),
        }
    }

    /// Get price and availability calendar for an Airbnb listing.
    #[tool(
        name = "airbnb_price_calendar",
        description = "Get price and availability calendar for an Airbnb listing showing daily prices, availability status, and minimum night requirements.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_price_calendar(
        &self,
        Parameters(params): Parameters<CalendarToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(3).clamp(1, 12);

        match self.client.get_price_calendar(&params.id, months).await {
            Ok(calendar) => Ok(CallToolResult::success(vec![Content::text(
                calendar.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get price calendar: {e}"
            ))])),
        }
    }

    /// Get the host profile for an Airbnb listing.
    #[tool(
        name = "airbnb_host_profile",
        description = "Get detailed host profile including superhost status, response rate, languages, bio, and listing count.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_host_profile(
        &self,
        Parameters(params): Parameters<HostProfileToolParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.get_host_profile(&params.id).await {
            Ok(profile) => Ok(CallToolResult::success(vec![Content::text(
                profile.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get host profile: {e}"
            ))])),
        }
    }

    /// Get aggregated neighborhood statistics from Airbnb listings.
    #[tool(
        name = "airbnb_neighborhood_stats",
        description = "Get aggregated statistics for a neighborhood: average/median prices, ratings, property type distribution, and superhost percentage.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_neighborhood_stats(
        &self,
        Parameters(params): Parameters<NeighborhoodStatsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let search_params = SearchParams {
            location: params.location,
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
            Ok(stats) => Ok(CallToolResult::success(vec![Content::text(
                stats.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get neighborhood stats: {e}"
            ))])),
        }
    }

    /// Get occupancy estimate for an Airbnb listing.
    #[tool(
        name = "airbnb_occupancy_estimate",
        description = "Estimate occupancy rate, average prices (weekday vs weekend), and monthly breakdown for a listing based on calendar data.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn airbnb_occupancy_estimate(
        &self,
        Parameters(params): Parameters<OccupancyEstimateToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let months = params.months.unwrap_or(3).clamp(1, 12);

        match self.client.get_occupancy_estimate(&params.id, months).await {
            Ok(estimate) => Ok(CallToolResult::success(vec![Content::text(
                estimate.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to get occupancy estimate: {e}"
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for AirbnbMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Airbnb MCP server for searching and browsing listings. \
                 Use airbnb_search to find listings, airbnb_listing_details for full info, \
                 airbnb_reviews for guest reviews, airbnb_price_calendar for pricing, \
                 airbnb_host_profile for host details, airbnb_neighborhood_stats for area analysis, \
                 and airbnb_occupancy_estimate for occupancy insights."
                    .into(),
            ),
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
                total_days: 0, occupied_days: 0, available_days: 0,
                occupancy_rate: 0.0, average_available_price: None,
                weekend_avg_price: None, weekday_avg_price: None,
                monthly_breakdown: vec![],
            })
        });
        let server = make_server(mock);

        // months=0 => clamped to 1
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "1".into(), months: Some(0),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("months=1"), "Expected months=1, got: {text}");

        // months=99 => clamped to 12
        let result = server
            .airbnb_occupancy_estimate(Parameters(OccupancyEstimateToolParams {
                id: "1".into(), months: Some(99),
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("months=12"), "Expected months=12, got: {text}");
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
                checkin: None, checkout: None, adults: None, children: None,
                infants: None, pets: None, min_price: None, max_price: None,
                property_type: None, cursor: None,
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("4.9"), "Should contain rating");
        assert!(text.contains("42 reviews"), "Should contain review count");
        assert!(text.contains("Entire villa"), "Should contain property type");
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
                checkin: None, checkout: None, adults: None, children: None,
                infants: None, pets: None, min_price: None, max_price: None,
                property_type: None, cursor: None,
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
                id: "42".into(), cursor: None,
            }))
            .await
            .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Eve"));
        assert!(text.contains("Loved it!"));
        assert!(text.contains("4.7"), "Should contain overall rating from summary");
        assert!(text.contains("More reviews available"), "Should contain pagination indicator");
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
    }
}
