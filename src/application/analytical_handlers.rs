//! Orchestration of every analytical tool: the fetches through
//! `AirbnbClient`, then the `domain::analytics::compute_*` call. This is
//! the single orchestration path. The `airbnb` CLI and the MCP server both
//! call these functions, so the two front ends make the same upstream calls
//! with the same defaults and limits (`tests/parity_test.rs`). Each
//! function validates its listing id, ranges and optional location override
//! before the first fetch.

use std::collections::HashSet;
use std::sync::Arc;

use crate::domain::analytics::{
    AmenityAnalysis, CompareListingsResult, CompetitivePositioning, GapFinderResult, HostPortfolio,
    ListingScore, MarketComparison, PriceTrends, PricingRecommendation, RevenueEstimate,
    ReviewSentiment, compute_amenity_analysis, compute_compare_listings,
    compute_competitive_positioning, compute_gap_finder, compute_host_portfolio,
    compute_listing_score, compute_market_comparison, compute_optimal_pricing,
    compute_price_trends, compute_revenue_estimate, compute_review_sentiment,
};
use crate::domain::limits;
use crate::domain::listing::Listing;
use crate::domain::listing_id::validate_listing_id;
use crate::domain::review::Review;
use crate::domain::search_params::{SEARCH_PAGE_SIZE, SearchParams};
use crate::error::{AirbnbError, Result, quote_input};
use crate::ports::airbnb_client::AirbnbClient;

/// What to compare.
#[derive(Debug, Clone)]
pub enum CompareTarget {
    /// 2–10 explicit listing ids.
    Ids(Vec<String>),
    /// The first `max_listings` results of a location search.
    Location(CompareSearch),
}

/// A location search that feeds a comparison.
#[derive(Debug, Clone)]
pub struct CompareSearch {
    /// Location to search.
    pub location: String,
    /// Most listings to compare (2–100).
    pub max_listings: u32,
    /// Check-in date (YYYY-MM-DD).
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD).
    pub checkout: Option<String>,
    /// Property type filter.
    pub property_type: Option<String>,
}

impl CompareSearch {
    /// Search parameters for one page of this discovery search.
    pub fn to_search_params(&self, cursor: Option<String>) -> SearchParams {
        SearchParams {
            location: self.location.clone(),
            checkin: self.checkin.clone(),
            checkout: self.checkout.clone(),
            property_type: self.property_type.clone(),
            cursor,
            ..SearchParams::default()
        }
    }
}

/// A comparison, plus how many search pages fed it (location mode only).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompareReport {
    /// The comparison itself.
    #[serde(flatten)]
    pub comparison: CompareListingsResult,
    /// Search pages fetched; `None` for an id comparison.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages_fetched: Option<u32>,
}

impl std::fmt::Display for CompareReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(pages) = self.pages_fetched {
            writeln!(
                f,
                "Fetched {} listings across {pages} page(s).\n",
                self.comparison.listings.len()
            )?;
        }
        write!(f, "{}", self.comparison)
    }
}

/// Compare listings: 2–10 explicit ids, or up to `max_listings` unique
/// listings from a paginated location search (`collect_search_pages`:
/// deduplicated by id, stops on a repeated cursor or a page without news).
pub async fn run_compare_listings(
    client: Arc<dyn AirbnbClient>,
    target: CompareTarget,
) -> Result<CompareReport> {
    // In id mode the fetched details feed bedrooms/amenity counts.
    let mut details = Vec::new();
    let (listings, pages_fetched) = match target {
        CompareTarget::Ids(ids) => {
            limits::count_in_range(
                "ids",
                ids.len(),
                limits::COMPARE_IDS_MIN,
                limits::COMPARE_IDS_MAX,
            )?;
            for id in &ids {
                validate_listing_id(id)?;
            }
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                let detail = client.get_listing_detail(id).await?;
                details.push(detail.clone());
                out.push(detail_to_listing(detail));
            }
            (out, None)
        }
        CompareTarget::Location(search) => {
            let max = limits::in_range(
                "max_listings",
                search.max_listings,
                limits::COMPARE_LISTINGS_MIN,
                limits::COMPARE_LISTINGS_MAX,
            )? as usize;
            let base = search.to_search_params(None);
            base.validate()?;
            let collected = collect_search_pages(client.as_ref(), &base, max).await?;
            (collected.listings, Some(collected.pages_fetched))
        }
    };

    // Only a location search can come back short (ids are counted above):
    // the input was valid, the market is too thin.
    if listings.len() < 2 {
        return Err(AirbnbError::InsufficientData {
            reason: format!(
                "need at least 2 listings to compare, found {}; try a broader location",
                listings.len()
            ),
        });
    }

    Ok(CompareReport {
        comparison: compute_compare_listings(
            &listings,
            (!details.is_empty()).then_some(details.as_slice()),
        ),
        pages_fetched,
    })
}

/// Listings gathered over several search pages, unique by listing id.
#[derive(Debug, Clone)]
pub struct PaginatedListings {
    pub listings: Vec<Listing>,
    pub pages_fetched: u32,
}

/// Page through `search_listings` from `base` until `max_listings` unique
/// listings are collected. Stops early when the upstream has no next page,
/// hands back a cursor it already returned, or serves a page without any new
/// listing id (what a cursor-ignoring upstream looks like).
pub async fn collect_search_pages(
    client: &dyn AirbnbClient,
    base: &SearchParams,
    max_listings: usize,
) -> Result<PaginatedListings> {
    let max_pages = max_listings.div_ceil(SEARCH_PAGE_SIZE).max(1);
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_cursors: HashSet<String> = HashSet::new();
    if let Some(ref start) = base.cursor {
        seen_cursors.insert(start.clone());
    }
    let mut listings: Vec<Listing> = Vec::new();
    let mut pages_fetched: u32 = 0;
    let mut cursor = base.cursor.clone();

    for _ in 0..max_pages {
        let params = SearchParams {
            cursor: cursor.clone(),
            ..base.clone()
        };
        let page = client.search_listings(&params).await?;
        pages_fetched += 1;

        let before = listings.len();
        for listing in page.listings {
            if seen_ids.insert(listing.id.clone()) {
                listings.push(listing);
            }
        }
        if listings.len() >= max_listings || listings.len() == before {
            break;
        }
        match page.next_cursor {
            Some(next) if seen_cursors.insert(next.clone()) => cursor = Some(next),
            _ => break,
        }
    }

    listings.truncate(max_listings);
    Ok(PaginatedListings {
        listings,
        pages_fetched,
    })
}

/// Review pages gathered for one listing, and how the walk ended.
#[derive(Debug, Clone)]
pub struct ReviewWalk {
    /// Reviews of every page fetched, in page order.
    pub reviews: Vec<Review>,
    /// Pages fetched successfully.
    pub pages_fetched: u32,
    /// The error that ended the walk after reviews had been collected; `None`
    /// when the walk ended normally (no next page, repeated cursor, `max_pages`).
    pub stopped_by: Option<String>,
}

/// Fetch up to `max_pages` review pages. Stops when the upstream has no next
/// page or repeats a cursor (P1a, MCP-1/PARSE-3). An error before any review
/// was collected is returned; a later error ends the walk, is logged, and is
/// reported in `stopped_by` together with the reviews gathered so far.
pub async fn walk_review_pages(
    client: &dyn AirbnbClient,
    id: &str,
    max_pages: u32,
) -> Result<ReviewWalk> {
    let mut reviews: Vec<Review> = Vec::new();
    let mut seen_cursors: HashSet<String> = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut pages_fetched = 0_u32;
    let mut stopped_by = None;

    for _ in 0..max_pages {
        let page = match client.get_reviews(id, cursor.as_deref()).await {
            Ok(page) => page,
            Err(e) if reviews.is_empty() => return Err(e),
            Err(e) => {
                tracing::warn!(
                    listing_id = %id.escape_debug(),
                    pages_fetched,
                    error = %e,
                    "stopping review pagination after an upstream error"
                );
                stopped_by = Some(e.to_string());
                break;
            }
        };
        pages_fetched += 1;
        reviews.extend(page.reviews);
        match page.next_cursor {
            Some(next) if seen_cursors.insert(next.clone()) => cursor = Some(next),
            _ => break,
        }
    }
    Ok(ReviewWalk {
        reviews,
        pages_fetched,
        stopped_by,
    })
}

/// Fetch up to `max_pages` review pages. Stops when the upstream has no next
/// page or repeats a cursor. An error before any review was collected is
/// returned; a later error ends pagination and keeps the reviews gathered so far.
pub async fn collect_review_pages(
    client: &dyn AirbnbClient,
    id: &str,
    max_pages: u32,
) -> Result<Vec<Review>> {
    Ok(walk_review_pages(client, id, max_pages).await?.reviews)
}

/// Check an optional location override with `limits::check_location`, the
/// rule the MCP handlers apply, so the CLI refuses the same input before the
/// first fetch (I8).
fn check_location_override(location: Option<&str>) -> Result<()> {
    location.map_or(Ok(()), limits::check_location)
}

/// Price trends over `months` months (1–12) of the listing's calendar.
pub async fn run_price_trends(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    months: u32,
) -> Result<PriceTrends> {
    validate_listing_id(id)?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    let calendar = client.get_price_calendar(id, months).await?;
    Ok(compute_price_trends(id, &calendar))
}

/// Booking gaps over `months` months (1–12) of the listing's calendar.
pub async fn run_gap_finder(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    months: u32,
) -> Result<GapFinderResult> {
    validate_listing_id(id)?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    let calendar = client.get_price_calendar(id, months).await?;
    Ok(compute_gap_finder(id, &calendar))
}

/// Revenue estimate for a listing or, without an id, for a location.
///
/// With an id, the detail and calendar are required and their errors
/// propagate. Occupancy is measured from the calendar. The neighborhood feeds
/// the ADR fallback and the comparison; its failure is ignored while the
/// calendar or the listing gives a price, and propagates otherwise (live, the
/// calendar publishes no prices and the undated detail has none, so the
/// neighborhood is usually the only ADR source).
///
/// Without an id, a location is required and only the neighborhood stats are
/// used (occupancy is then a labelled assumption).
pub async fn run_revenue_estimate(
    client: Arc<dyn AirbnbClient>,
    id: Option<&str>,
    location: Option<String>,
    months: u32,
) -> Result<RevenueEstimate> {
    if let Some(id) = id {
        validate_listing_id(id)?;
    }
    check_location_override(location.as_deref())?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    if let Some(id) = id {
        let detail = client.get_listing_detail(id).await?;
        let location = location.unwrap_or_else(|| detail.location.clone());
        let calendar = client.get_price_calendar(id, months).await?;
        let sp = SearchParams {
            location: location.clone(),
            ..SearchParams::default()
        };
        let (neighborhood, search_err) = match client.get_neighborhood_stats(&sp).await {
            Ok(stats) => (Some(stats), None),
            Err(e) => (None, Some(e)),
        };

        let computed = compute_revenue_estimate(
            Some(id),
            &location,
            Some(&detail),
            Some(&calendar),
            neighborhood.as_ref(),
        );
        settle_without_neighborhood(id, computed, search_err, detail.known_price().is_some())
    } else {
        let location = location.filter(|l| !l.trim().is_empty()).ok_or_else(|| {
            AirbnbError::InvalidParams {
                reason: "provide a listing id or a location".into(),
            }
        })?;
        let sp = SearchParams {
            location: location.clone(),
            ..SearchParams::default()
        };
        sp.validate()?;
        let neighborhood = client.get_neighborhood_stats(&sp).await?;
        compute_revenue_estimate(None, &location, None, None, Some(&neighborhood))
    }
}

/// Amenity analysis: compares a listing's amenities against a sample of
/// comparable listings fetched via search + per-listing details. Fails when
/// the search fails or no comparable with amenity data can be fetched.
pub async fn run_amenity_analysis(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
) -> Result<AmenityAnalysis> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location: location.clone(),
        ..SearchParams::default()
    };
    let neighbor_ids: Vec<String> = client
        .search_listings(&sp)
        .await?
        .listings
        .into_iter()
        .filter(|l| l.id != id)
        .take(5)
        .map(|l| l.id)
        .collect();

    let mut neighbor_details = Vec::new();
    for nid in &neighbor_ids {
        if let Ok(d) = client.get_listing_detail(nid).await {
            neighbor_details.push(d);
        }
    }

    let analysis = compute_amenity_analysis(&detail, &neighbor_details);
    if analysis.comparables_analyzed == 0 {
        return Err(AirbnbError::InsufficientData {
            reason: format!(
                "no comparable listing with amenity data could be fetched for {} ({} candidate(s) found)",
                quote_input(&location),
                neighbor_ids.len()
            ),
        });
    }
    Ok(analysis)
}

/// Host portfolio: the queried listing plus the listings by the same host
/// found on the first search page for its location (see
/// `compute_host_portfolio`). A search failure is an error.
pub async fn run_host_portfolio(
    client: Arc<dyn AirbnbClient>,
    listing_id: &str,
) -> Result<HostPortfolio> {
    validate_listing_id(listing_id)?;
    let detail = client.get_listing_detail(listing_id).await?;
    let sp = SearchParams {
        location: detail.location.clone(),
        ..SearchParams::default()
    };
    let candidates = client.search_listings(&sp).await?.listings;
    Ok(compute_host_portfolio(
        &detail,
        &detail.location,
        &candidates,
    ))
}

/// Competitive positioning: ranks the listing against the comparable
/// listings of a location search. A search failure is an error, while the
/// occupancy and amenity inputs are optional.
pub async fn run_competitive_positioning(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
) -> Result<CompetitivePositioning> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location,
        ..SearchParams::default()
    };
    let comparables: Vec<Listing> = client
        .search_listings(&sp)
        .await?
        .listings
        .into_iter()
        .filter(|l| l.id != id)
        .collect();
    let occupancy = client.get_occupancy_estimate(id, 3).await.ok();

    let mut neighbor_details = Vec::new();
    for listing in comparables.iter().take(5) {
        if let Ok(d) = client.get_listing_detail(&listing.id).await {
            neighbor_details.push(d);
        }
    }
    let amenity_analysis = compute_amenity_analysis(&detail, &neighbor_details);

    Ok(compute_competitive_positioning(
        &detail,
        &comparables,
        occupancy.as_ref(),
        Some(&amenity_analysis),
    ))
}

/// Optimal pricing: combines neighborhood stats, price trends, and amenity
/// analysis to produce a recommendation with reasoning. A neighborhood
/// search failure is ignored while the listing has a known price, and
/// propagates otherwise (live, the undated detail has no price, so the
/// neighborhood median is usually the only baseline).
pub async fn run_optimal_pricing(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
    months: u32,
) -> Result<PricingRecommendation> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location,
        ..SearchParams::default()
    };
    let (neighborhood, search_err) = match client.get_neighborhood_stats(&sp).await {
        Ok(stats) => (Some(stats), None),
        Err(e) => (None, Some(e)),
    };
    let price_trends = match client.get_price_calendar(id, months).await {
        Ok(calendar) => Some(compute_price_trends(id, &calendar)),
        Err(_) => None,
    };

    let amenity_analysis = match client.search_listings(&sp).await {
        Ok(search_result) => {
            let neighbor_ids: Vec<String> = search_result
                .listings
                .into_iter()
                .filter(|l| l.id != id)
                .take(5)
                .map(|l| l.id)
                .collect();
            let mut neighbor_details = Vec::new();
            for nid in &neighbor_ids {
                if let Ok(d) = client.get_listing_detail(nid).await {
                    neighbor_details.push(d);
                }
            }
            Some(compute_amenity_analysis(&detail, &neighbor_details))
        }
        Err(_) => None,
    };

    let computed = compute_optimal_pricing(
        &detail,
        neighborhood.as_ref(),
        price_trends.as_ref(),
        amenity_analysis.as_ref(),
    );
    settle_without_neighborhood(id, computed, search_err, detail.known_price().is_some())
}

/// Settle a computation that may have run without the neighborhood stats.
///
/// Without a listing price, the failed neighborhood search was the only price
/// baseline left, so its error is the real cause and is returned instead of
/// the computation's. Otherwise the search failure is dropped and logged
/// (CLI-2: a dropped partial failure leaves a trace on stderr).
fn settle_without_neighborhood<T>(
    id: &str,
    computed: Result<T>,
    search_err: Option<AirbnbError>,
    listing_has_price: bool,
) -> Result<T> {
    let Some(search) = search_err else {
        return computed;
    };
    match computed {
        Err(_) if !listing_has_price => Err(search),
        other => {
            tracing::warn!(
                listing_id = %id.escape_debug(),
                error = %search,
                "neighborhood stats unavailable; continuing without them"
            );
            other
        }
    }
}

/// Review sentiment, plus how the page walk ended.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewSentimentReport {
    /// The sentiment analysis of the reviews that were fetched.
    #[serde(flatten)]
    pub sentiment: ReviewSentiment,
    /// Review pages actually fetched.
    pub pages_fetched: u32,
    /// Why the walk stopped early when a later page failed; `None` when complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl std::fmt::Display for ReviewSentimentReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.sentiment)?;
        writeln!(f, "\nReview pages fetched: {}", self.pages_fetched)?;
        if let Some(ref warning) = self.warning {
            writeln!(f, "Warning: {warning}")?;
        }
        Ok(())
    }
}

/// Review sentiment over up to `max_pages` pages (1–20). A failure before any
/// review is an error. A later failure keeps the pages already fetched, is
/// logged by `walk_review_pages`, and is recorded in `warning` (CLI-2).
pub async fn run_review_sentiment(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    max_pages: u32,
) -> Result<ReviewSentimentReport> {
    validate_listing_id(id)?;
    let max_pages = limits::in_range(
        "max_pages",
        max_pages,
        limits::REVIEW_PAGES_MIN,
        limits::REVIEW_PAGES_MAX,
    )?;
    let walk = walk_review_pages(client.as_ref(), id, max_pages).await?;
    let warning = walk
        .stopped_by
        .map(|e| format!("stopped after {} page(s): {e}", walk.pages_fetched));
    Ok(ReviewSentimentReport {
        sentiment: compute_review_sentiment(id, &walk.reviews),
        pages_fetched: walk.pages_fetched,
        warning,
    })
}

/// Listing score. Neighborhood stats only refine the pricing category, so a
/// failure there is logged and the score is computed without them.
pub async fn run_listing_score(client: Arc<dyn AirbnbClient>, id: &str) -> Result<ListingScore> {
    validate_listing_id(id)?;
    let detail = client.get_listing_detail(id).await?;
    let sp = SearchParams {
        location: detail.location.clone(),
        ..SearchParams::default()
    };
    let neighborhood = match client.get_neighborhood_stats(&sp).await {
        Ok(stats) => Some(stats),
        Err(e) => {
            tracing::warn!(
                listing_id = %id.escape_debug(),
                error = %e,
                "neighborhood stats unavailable; scoring without the pricing comparison"
            );
            None
        }
    };
    Ok(compute_listing_score(&detail, neighborhood.as_ref()))
}

/// Convert a `ListingDetail` to a lightweight `Listing` for use in
/// compare-listings when we only have details (fetched by IDs).
fn detail_to_listing(d: crate::domain::listing::ListingDetail) -> Listing {
    Listing {
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
    }
}

/// Inputs of a market comparison: 2–5 locations and shared search filters.
#[derive(Debug, Clone)]
pub struct MarketRequest {
    /// Locations to compare, one market each.
    pub locations: Vec<String>,
    /// Check-in date (YYYY-MM-DD) applied to every market.
    pub checkin: Option<String>,
    /// Check-out date (YYYY-MM-DD) applied to every market.
    pub checkout: Option<String>,
    /// Property type filter applied to every market.
    pub property_type: Option<String>,
}

/// Market comparison: neighborhood statistics for each location, side by
/// side. Every location is validated before the first fetch.
pub async fn run_market_comparison(
    client: Arc<dyn AirbnbClient>,
    request: MarketRequest,
) -> Result<MarketComparison> {
    limits::count_in_range(
        "locations",
        request.locations.len(),
        limits::MARKET_LOCATIONS_MIN,
        limits::MARKET_LOCATIONS_MAX,
    )?;
    let searches: Vec<SearchParams> = request
        .locations
        .iter()
        .map(|location| SearchParams {
            location: location.trim().to_string(),
            checkin: request.checkin.clone(),
            checkout: request.checkout.clone(),
            property_type: request.property_type.clone(),
            ..SearchParams::default()
        })
        .collect();
    for search in &searches {
        search.validate()?;
    }
    let mut stats = Vec::with_capacity(searches.len());
    for search in &searches {
        stats.push(client.get_neighborhood_stats(search).await?);
    }
    Ok(compute_market_comparison(&stats))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::analytics::{DataSource, HostMatch};
    use crate::domain::listing::SearchResult;
    use crate::test_helpers::{
        MockAirbnbClient, make_calendar_day, make_listing, make_listing_detail,
        make_price_calendar, make_review, make_reviews_page, make_search_result,
    };

    fn page(ids: &[&str], next: Option<&str>) -> SearchResult {
        SearchResult {
            listings: ids
                .iter()
                .map(|id| make_listing(id, &format!("Listing {id}"), 100.0))
                .collect(),
            total_count: None,
            next_cursor: next.map(String::from),
        }
    }

    fn paris() -> SearchParams {
        SearchParams {
            location: "Paris".into(),
            ..SearchParams::default()
        }
    }

    #[tokio::test]
    async fn collect_search_pages_follows_distinct_cursors_and_dedups() {
        let mock = MockAirbnbClient::new().with_search(|params| {
            Ok(match params.cursor.as_deref() {
                None => page(&["1", "2"], Some("c2")),
                Some("c2") => page(&["2", "3"], Some("c3")),
                Some(_) => page(&["4"], None),
            })
        });
        let collected = collect_search_pages(&mock, &paris(), 60).await.unwrap();
        let ids: Vec<&str> = collected.listings.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3", "4"]);
        assert_eq!(collected.pages_fetched, 3);
    }

    #[tokio::test]
    async fn collect_search_pages_stops_when_upstream_repeats_a_page() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let mock = MockAirbnbClient::new().with_search(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(page(&["1", "2", "3"], Some("same")))
        });
        let collected = collect_search_pages(&mock, &paris(), 100).await.unwrap();
        assert_eq!(collected.listings.len(), 3);
        assert_eq!(collected.pages_fetched, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn collect_search_pages_truncates_to_max() {
        let mock = MockAirbnbClient::new().with_search(|params| {
            Ok(match params.cursor.as_deref() {
                None => page(&["1", "2", "3"], Some("c2")),
                Some(_) => page(&["4", "5", "6"], None),
            })
        });
        let collected = collect_search_pages(&mock, &paris(), 2).await.unwrap();
        assert_eq!(collected.listings.len(), 2);
        assert_eq!(collected.pages_fetched, 1);
    }

    #[tokio::test]
    async fn collect_search_pages_propagates_errors() {
        let mock = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let err = collect_search_pages(&mock, &paris(), 20).await.unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited));
    }

    #[tokio::test]
    async fn collect_review_pages_stops_on_a_repeated_cursor() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let mock = MockAirbnbClient::new().with_reviews(move |id, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut p = make_reviews_page(id, vec![make_review("Guest", "Nice")]);
            p.next_cursor = Some("24".into());
            Ok(p)
        });
        let reviews = collect_review_pages(&mock, "42", 5).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(reviews.len(), 2);
    }

    #[tokio::test]
    async fn collect_review_pages_follows_advancing_offsets() {
        let mock = MockAirbnbClient::new().with_reviews(|id, cursor| {
            let (next, author) = match cursor {
                None => (Some("24"), "p1"),
                Some("24") => (Some("48"), "p2"),
                Some(_) => (None, "p3"),
            };
            let mut p = make_reviews_page(id, vec![make_review(author, "Nice")]);
            p.next_cursor = next.map(String::from);
            Ok(p)
        });
        let reviews = collect_review_pages(&mock, "42", 5).await.unwrap();
        let authors: Vec<&str> = reviews.iter().map(|r| r.author.as_str()).collect();
        assert_eq!(authors, ["p1", "p2", "p3"]);
    }

    #[tokio::test]
    async fn collect_review_pages_keeps_earlier_pages_when_a_later_page_fails() {
        let mock = MockAirbnbClient::new().with_reviews(|id, cursor| match cursor {
            None => {
                let mut p = make_reviews_page(id, vec![make_review("A", "Nice")]);
                p.next_cursor = Some("24".into());
                Ok(p)
            }
            Some(_) => Err(AirbnbError::RateLimited),
        });
        let reviews = collect_review_pages(&mock, "42", 5).await.unwrap();
        assert_eq!(reviews.len(), 1);
    }

    #[tokio::test]
    async fn collect_review_pages_returns_a_first_page_error() {
        let mock = MockAirbnbClient::new().with_reviews(|_, _| Err(AirbnbError::RateLimited));
        assert!(collect_review_pages(&mock, "42", 5).await.is_err());
    }

    #[tokio::test]
    async fn amenity_analysis_without_comparables_is_insufficient_data() {
        let client: Arc<dyn AirbnbClient> = Arc::new(
            MockAirbnbClient::new()
                .with_search(|_| Ok(make_search_result(vec![make_listing("42", "Self", 100.0)]))),
        );
        let err = run_amenity_analysis(client, "42", None).await.unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    #[tokio::test]
    async fn amenity_analysis_search_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited)));
        let err = run_amenity_analysis(client, "42", None).await.unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn revenue_estimate_calendar_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_calendar(|_, _| Err(AirbnbError::RateLimited)));
        let err = run_revenue_estimate(client, Some("42"), Some("Paris".into()), 12)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn revenue_estimate_labels_measured_occupancy() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_calendar(|id, _| {
                Ok(make_price_calendar(
                    id,
                    vec![
                        make_calendar_day("2099-01-01", Some(100.0), true),
                        make_calendar_day("2099-01-02", Some(100.0), false),
                    ],
                ))
            }));
        let estimate = run_revenue_estimate(client, Some("42"), None, 12)
            .await
            .unwrap();
        assert_eq!(estimate.occupancy_source, DataSource::Calendar);
        assert!((estimate.projected_occupancy_pct - 50.0).abs() < 1e-9);
    }

    /// Live shape: future calendar nights without prices.
    fn unpriced_future_calendar(id: &str) -> crate::domain::calendar::PriceCalendar {
        make_price_calendar(
            id,
            vec![
                make_calendar_day("2099-01-01", None, true),
                make_calendar_day("2099-01-02", None, false),
            ],
        )
    }

    #[tokio::test]
    async fn revenue_estimate_neighborhood_failure_propagates_when_no_price_is_known() {
        let client: Arc<dyn AirbnbClient> = Arc::new(
            MockAirbnbClient::new()
                .with_detail(|id| {
                    let mut detail = make_listing_detail(id);
                    detail.price_per_night = 0.0;
                    Ok(detail)
                })
                .with_calendar(|id, _| Ok(unpriced_future_calendar(id)))
                .with_neighborhood(|_| Err(AirbnbError::RateLimited)),
        );
        let err = run_revenue_estimate(client, Some("42"), Some("Paris".into()), 12)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn revenue_estimate_neighborhood_failure_is_not_fatal_with_a_listing_price() {
        let client: Arc<dyn AirbnbClient> = Arc::new(
            MockAirbnbClient::new()
                .with_calendar(|id, _| Ok(unpriced_future_calendar(id)))
                .with_neighborhood(|_| Err(AirbnbError::RateLimited)),
        );
        let estimate = run_revenue_estimate(client, Some("42"), Some("Paris".into()), 12)
            .await
            .unwrap();
        assert_eq!(estimate.adr_source, DataSource::ListingPrice);
        assert!((estimate.projected_adr - 100.0).abs() < 1e-9);
        assert!(estimate.vs_neighborhood_avg_price_pct.is_none());
    }

    #[tokio::test]
    async fn optimal_pricing_without_any_price_is_insufficient_data() {
        let client: Arc<dyn AirbnbClient> = Arc::new(MockAirbnbClient::new().with_detail(|id| {
            let mut detail = make_listing_detail(id);
            detail.price_per_night = 0.0;
            Ok(detail)
        }));
        let err = run_optimal_pricing(client, "42", None, 12)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    #[tokio::test]
    async fn optimal_pricing_neighborhood_failure_propagates_when_no_price_is_known() {
        // Live shape: the undated detail has no price, so the neighborhood
        // median is the only baseline. A failed search must surface as
        // itself (retryable), not as "Insufficient data".
        let client: Arc<dyn AirbnbClient> = Arc::new(
            MockAirbnbClient::new()
                .with_detail(|id| {
                    let mut detail = make_listing_detail(id);
                    detail.price_per_night = 0.0;
                    Ok(detail)
                })
                .with_neighborhood(|_| Err(AirbnbError::RateLimited)),
        );
        let err = run_optimal_pricing(client, "42", Some("Paris".into()), 12)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn optimal_pricing_neighborhood_failure_is_not_fatal_with_a_listing_price() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited)));
        let recommendation = run_optimal_pricing(client, "42", Some("Paris".into()), 12)
            .await
            .unwrap();
        assert_eq!(recommendation.current_price, Some(100.0));
        assert!(recommendation.vs_neighborhood_median.is_none());
        assert!(
            recommendation
                .reasoning
                .iter()
                .any(|r| r.contains("current listing price")),
            "{:?}",
            recommendation.reasoning
        );
    }

    #[tokio::test]
    async fn competitive_positioning_search_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited)));
        let err = run_competitive_positioning(client, "42", None)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn host_portfolio_search_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited)));
        let err = run_host_portfolio(client, "42").await.unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn host_portfolio_keeps_queried_listing() {
        let client: Arc<dyn AirbnbClient> = Arc::new(MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "43", "Sibling", 90.0,
            )]))
        }));
        let portfolio = run_host_portfolio(client, "42").await.unwrap();
        let ids: Vec<&str> = portfolio.properties.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["42", "43"]);
        assert_eq!(portfolio.matched_by, HostMatch::HostName);
    }
}

#[cfg(test)]
mod contract_tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::test_helpers::*;

    #[tokio::test]
    async fn market_comparison_validates_every_location_before_fetching() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mock = MockAirbnbClient::new().with_neighborhood(move |p| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(make_neighborhood_stats(&p.location))
        });
        let request = MarketRequest {
            locations: vec!["Paris".into(), "   ".into()],
            checkin: None,
            checkout: None,
            property_type: None,
        };
        assert!(
            run_market_comparison(Arc::new(mock), request)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn market_comparison_forwards_filters_and_rejects_six() {
        let seen = Arc::new(Mutex::new(Vec::<SearchParams>::new()));
        let log = Arc::clone(&seen);
        let mock = MockAirbnbClient::new().with_neighborhood(move |p| {
            log.lock().unwrap().push(p.clone());
            Ok(make_neighborhood_stats(&p.location))
        });
        let client: Arc<dyn AirbnbClient> = Arc::new(mock);
        let request = MarketRequest {
            locations: vec!["Paris, France".into(), "Lyon, France".into()],
            checkin: None,
            checkout: None,
            property_type: Some("Entire home".into()),
        };
        let result = run_market_comparison(Arc::clone(&client), request)
            .await
            .unwrap();
        assert_eq!(result.locations.len(), 2);
        let calls = seen.lock().unwrap().clone();
        assert_eq!(calls[0].location, "Paris, France");
        assert_eq!(calls[1].property_type.as_deref(), Some("Entire home"));

        let six = MarketRequest {
            locations: ["A", "B", "C", "D", "E", "F"].map(String::from).to_vec(),
            checkin: None,
            checkout: None,
            property_type: None,
        };
        assert!(run_market_comparison(client, six).await.is_err());
    }

    fn listing_page(ids: &[&str], next: Option<&str>) -> crate::domain::listing::SearchResult {
        let mut page = make_search_result(
            ids.iter()
                .map(|id| make_listing(id, "Flat", 100.0))
                .collect(),
        );
        page.next_cursor = next.map(str::to_string);
        page
    }

    #[tokio::test]
    async fn compare_by_location_paginates_and_dedupes() {
        let mock = MockAirbnbClient::new().with_search(|p| {
            Ok(match p.cursor.as_deref() {
                None => listing_page(&["1", "2", "3"], Some("p2")),
                Some("p2") => listing_page(&["3", "4", "5"], Some("p3")),
                Some(_) => listing_page(&["5", "6"], None),
            })
        });
        let search = CompareSearch {
            location: "Rome".into(),
            max_listings: 60,
            checkin: None,
            checkout: None,
            property_type: None,
        };
        let report = run_compare_listings(Arc::new(mock), CompareTarget::Location(search))
            .await
            .unwrap();
        assert_eq!(report.comparison.listings.len(), 6);
        assert_eq!(report.pages_fetched, Some(3));
    }

    #[tokio::test]
    async fn compare_by_location_stops_when_a_page_repeats() {
        let mock =
            MockAirbnbClient::new().with_search(|_| Ok(listing_page(&["1", "2", "3"], Some("p2"))));
        let search = CompareSearch {
            location: "Rome".into(),
            max_listings: 100,
            checkin: None,
            checkout: None,
            property_type: None,
        };
        let report = run_compare_listings(Arc::new(mock), CompareTarget::Location(search))
            .await
            .unwrap();
        assert_eq!(report.comparison.listings.len(), 3);
        assert_eq!(report.pages_fetched, Some(2));
    }

    #[tokio::test]
    async fn compare_rejects_eleven_ids_without_fetching() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mock = MockAirbnbClient::new().with_detail(move |id| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(make_listing_detail(id))
        });
        let ids = (1..=11).map(|i| i.to_string()).collect();
        assert!(
            run_compare_listings(Arc::new(mock), CompareTarget::Ids(ids))
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn compare_by_location_forwards_filters() {
        let seen = Arc::new(Mutex::new(Vec::<SearchParams>::new()));
        let log = Arc::clone(&seen);
        let mock = MockAirbnbClient::new().with_search(move |p| {
            log.lock().unwrap().push(p.clone());
            Ok(listing_page(&["1", "2"], None))
        });
        let checkin = (chrono::Utc::now().date_naive() + chrono::Days::new(30))
            .format("%Y-%m-%d")
            .to_string();
        let checkout = (chrono::Utc::now().date_naive() + chrono::Days::new(33))
            .format("%Y-%m-%d")
            .to_string();
        let search = CompareSearch {
            location: "Rome".into(),
            max_listings: 20,
            checkin: Some(checkin.clone()),
            checkout: Some(checkout),
            property_type: Some("Entire home".into()),
        };
        run_compare_listings(Arc::new(mock), CompareTarget::Location(search))
            .await
            .unwrap();
        let calls = seen.lock().unwrap().clone();
        assert_eq!(calls[0].checkin.as_deref(), Some(checkin.as_str()));
        assert_eq!(calls[0].property_type.as_deref(), Some("Entire home"));
    }

    #[tokio::test]
    async fn revenue_by_location_only_uses_neighborhood_stats() {
        let mock = MockAirbnbClient::new().with_neighborhood(|p| {
            let mut stats = make_neighborhood_stats(&p.location);
            stats.average_price = Some(140.0);
            stats.total_listings = 30;
            Ok(stats)
        });
        let estimate = run_revenue_estimate(Arc::new(mock), None, Some("Rome, Italy".into()), 12)
            .await
            .unwrap();
        assert!(estimate.listing_id.is_none());
        assert_eq!(estimate.location, "Rome, Italy");
    }

    #[tokio::test]
    async fn revenue_requires_an_id_or_a_location() {
        let err = run_revenue_estimate(Arc::new(MockAirbnbClient::new()), None, None, 12)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("provide a listing id or a location")
        );
    }

    #[tokio::test]
    async fn review_sentiment_reports_a_partial_walk() {
        let mock = MockAirbnbClient::new().with_reviews(|id, cursor| match cursor {
            None => {
                let mut page =
                    make_reviews_page(id, vec![make_review("Guest A", "Clean and quiet.")]);
                page.next_cursor = Some("1".into());
                Ok(page)
            }
            Some(_) => Err(AirbnbError::RateLimited),
        });
        let report = run_review_sentiment(Arc::new(mock), "42", 5).await.unwrap();
        assert_eq!(report.pages_fetched, 1);
        let warning = report.warning.clone().expect("partial walk is flagged");
        assert!(warning.contains("stopped after 1 page(s)"), "{warning}");
        assert!(
            report
                .to_string()
                .contains("Warning: stopped after 1 page(s)")
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["listing_id"], "42");
        assert!(json["warning"].is_string());
    }

    #[tokio::test]
    async fn review_sentiment_fails_when_the_first_page_fails() {
        let mock = MockAirbnbClient::new().with_reviews(|_, _| Err(AirbnbError::RateLimited));
        assert!(run_review_sentiment(Arc::new(mock), "42", 5).await.is_err());
    }

    #[tokio::test]
    async fn review_sentiment_keeps_p1a_stop_on_a_repeated_cursor() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mock = MockAirbnbClient::new().with_reviews(move |id, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut page = make_reviews_page(id, vec![make_review("Guest A", "Clean.")]);
            page.next_cursor = Some("24".into());
            Ok(page)
        });
        let report = run_review_sentiment(Arc::new(mock), "42", 5).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(report.pages_fetched, 2);
        assert!(report.warning.is_none());
    }

    #[tokio::test]
    async fn listing_score_survives_missing_neighborhood_stats() {
        let mock = MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited));
        let score = run_listing_score(Arc::new(mock), "42").await.unwrap();
        assert_eq!(score.listing_id, "42");
    }

    #[tokio::test]
    async fn every_run_function_checks_its_inputs_before_fetching() {
        let details = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&details);
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_detail(move |id| {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(make_listing_detail(id))
            }));
        assert!(
            run_price_trends(Arc::clone(&client), "42", 12)
                .await
                .is_ok()
        );
        assert!(
            run_price_trends(Arc::clone(&client), "42", 13)
                .await
                .is_err()
        );
        assert!(run_gap_finder(Arc::clone(&client), "abc", 3).await.is_err());
        assert!(
            run_amenity_analysis(Arc::clone(&client), "0042", None)
                .await
                .is_err()
        );
        assert!(
            run_host_portfolio(Arc::clone(&client), "0042")
                .await
                .is_err()
        );
        assert!(
            run_competitive_positioning(Arc::clone(&client), "abc", None)
                .await
                .is_err()
        );
        assert!(
            run_optimal_pricing(Arc::clone(&client), "abc", None, 12)
                .await
                .is_err()
        );
        assert!(
            run_optimal_pricing(Arc::clone(&client), "42", None, 13)
                .await
                .is_err()
        );
        assert_eq!(
            details.load(Ordering::SeqCst),
            0,
            "no listing detail may be fetched for invalid input"
        );
    }

    /// A mock that counts every upstream call, whatever its kind.
    fn counting_client(calls: &Arc<AtomicUsize>) -> Arc<dyn AirbnbClient> {
        let (details, calendars, searches, stats, occupancy) = (
            Arc::clone(calls),
            Arc::clone(calls),
            Arc::clone(calls),
            Arc::clone(calls),
            Arc::clone(calls),
        );
        Arc::new(
            MockAirbnbClient::new()
                .with_detail(move |id| {
                    details.fetch_add(1, Ordering::SeqCst);
                    Ok(make_listing_detail(id))
                })
                .with_calendar(move |id, _| {
                    calendars.fetch_add(1, Ordering::SeqCst);
                    Ok(make_price_calendar(id, vec![]))
                })
                .with_search(move |_| {
                    searches.fetch_add(1, Ordering::SeqCst);
                    Ok(make_search_result(vec![]))
                })
                .with_neighborhood(move |p| {
                    stats.fetch_add(1, Ordering::SeqCst);
                    Ok(make_neighborhood_stats(&p.location))
                })
                .with_occupancy(move |id, _| {
                    occupancy.fetch_add(1, Ordering::SeqCst);
                    Ok(make_occupancy_estimate(id))
                }),
        )
    }

    /// Optional location overrides that `limits::check_location` refuses:
    /// blank, too long, a control character, no letter or digit.
    fn invalid_locations() -> Vec<String> {
        vec![
            String::new(),
            "   ".into(),
            "a".repeat(limits::LOCATION_MAX_CHARS + 1),
            "Paris\nINFO forged".into(),
            "../..".into(),
        ]
    }

    fn assert_invalid_params<T: std::fmt::Debug>(what: &str, location: &str, result: Result<T>) {
        match result {
            Err(AirbnbError::InvalidParams { .. }) => {}
            other => {
                panic!("{what} with location {location:?}: expected InvalidParams, got {other:?}")
            }
        }
    }

    #[tokio::test]
    async fn revenue_estimate_checks_the_location_override_before_fetching() {
        for location in invalid_locations() {
            let calls = Arc::new(AtomicUsize::new(0));
            let result = run_revenue_estimate(
                counting_client(&calls),
                Some("42"),
                Some(location.clone()),
                12,
            )
            .await;
            assert_invalid_params("revenue", &location, result);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "revenue fetched for {location:?}"
            );
        }
    }

    #[tokio::test]
    async fn amenity_analysis_checks_the_location_override_before_fetching() {
        for location in invalid_locations() {
            let calls = Arc::new(AtomicUsize::new(0));
            let result =
                run_amenity_analysis(counting_client(&calls), "42", Some(location.clone())).await;
            assert_invalid_params("amenities", &location, result);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "amenities fetched for {location:?}"
            );
        }
    }

    #[tokio::test]
    async fn competitive_positioning_checks_the_location_override_before_fetching() {
        for location in invalid_locations() {
            let calls = Arc::new(AtomicUsize::new(0));
            let result =
                run_competitive_positioning(counting_client(&calls), "42", Some(location.clone()))
                    .await;
            assert_invalid_params("competitive", &location, result);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "competitive fetched for {location:?}"
            );
        }
    }

    #[tokio::test]
    async fn optimal_pricing_checks_the_location_override_before_fetching() {
        for location in invalid_locations() {
            let calls = Arc::new(AtomicUsize::new(0));
            let result =
                run_optimal_pricing(counting_client(&calls), "42", Some(location.clone()), 12)
                    .await;
            assert_invalid_params("optimal pricing", &location, result);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "pricing fetched for {location:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_valid_location_override_is_still_used() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = Arc::clone(&seen);
        let client: Arc<dyn AirbnbClient> = Arc::new(
            MockAirbnbClient::new()
                .with_neighborhood(move |p| {
                    log.lock().unwrap().push(p.location.clone());
                    Ok(make_neighborhood_stats(&p.location))
                })
                .with_calendar(|id, _| {
                    Ok(make_price_calendar(
                        id,
                        vec![make_calendar_day("2099-01-01", Some(100.0), true)],
                    ))
                }),
        );
        run_optimal_pricing(Arc::clone(&client), "42", Some("Lyon, France".into()), 12)
            .await
            .unwrap();
        run_revenue_estimate(client, Some("42"), Some("Lyon, France".into()), 12)
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), ["Lyon, France", "Lyon, France"]);
    }

    #[tokio::test]
    async fn compare_by_location_with_one_result_is_insufficient_data() {
        let mock = MockAirbnbClient::new().with_search(|_| Ok(listing_page(&["1"], None)));
        let search = CompareSearch {
            location: "Tiny Village".into(),
            max_listings: 20,
            checkin: None,
            checkout: None,
            property_type: None,
        };
        let err = run_compare_listings(Arc::new(mock), CompareTarget::Location(search))
            .await
            .unwrap_err();
        match err {
            AirbnbError::InsufficientData { ref reason } => {
                assert!(reason.contains("need at least 2 listings"), "{reason}");
                assert!(reason.contains("found 1"), "{reason}");
                assert!(reason.contains("broader location"), "{reason}");
            }
            other => panic!("expected InsufficientData, got {other:?}"),
        }
    }
}
