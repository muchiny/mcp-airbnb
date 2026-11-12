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
use crate::domain::listing::{Listing, ListingDetail};
use crate::domain::listing_id::validate_listing_id;
use crate::domain::quote::{QuoteRequest, QuoteSeries};
use crate::domain::rank::{ListingRank, RankScan, compute_listing_rank};
use crate::domain::review::Review;
use crate::domain::search_params::{SEARCH_PAGE_SIZE, SearchFilters, SearchParams};
use crate::domain::status::{Probe, ProbeState, StatusReport};
use crate::error::{AirbnbError, Result, quote_input};
use crate::ports::airbnb_client::AirbnbClient;

/// The stay and the panel filters an analysis applies to the search of its
/// comparable listings. The default is an undated, unfiltered search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StayFilter {
    /// Check-in date, `YYYY-MM-DD` (with `checkout`).
    pub checkin: Option<String>,
    /// Check-out date, `YYYY-MM-DD` (with `checkin`).
    pub checkout: Option<String>,
    /// Number of adults.
    pub adults: Option<u32>,
    /// Number of children.
    pub children: Option<u32>,
    /// Type of place: "Entire home", "Private room" or "Hotel room".
    pub property_type: Option<String>,
    /// Rooms, amenities, Guest Favorite, Instant Book.
    pub filters: SearchFilters,
}

impl StayFilter {
    /// The search of `location` with this stay and these filters.
    pub fn search(&self, location: String) -> SearchParams {
        SearchParams {
            location,
            checkin: self.checkin.clone(),
            checkout: self.checkout.clone(),
            adults: self.adults,
            children: self.children,
            property_type: self.property_type.clone(),
            filters: self.filters.clone(),
            ..SearchParams::default()
        }
    }

    /// Check the stay and the filters with the search rules, before any
    /// fetch and before the location is known.
    pub fn validate(&self) -> Result<()> {
        self.search("stay".to_string()).validate()
    }

    /// The stay as a price quote request for `listing_id`, when it has
    /// dates. A quote needs at least one adult: 1 when none is given.
    pub fn quote_request(&self, listing_id: &str) -> Option<QuoteRequest> {
        Some(QuoteRequest {
            listing_id: listing_id.to_string(),
            checkin: self.checkin.clone()?,
            checkout: self.checkout.clone()?,
            adults: self.adults.unwrap_or(1).max(1),
        })
    }

    /// The stay and the filters in words, for report headers.
    pub fn describe(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let (Some(checkin), Some(checkout)) = (&self.checkin, &self.checkout) {
            parts.push(format!("{checkin} to {checkout}"));
        }
        if let Some(adults) = self.adults {
            parts.push(format!("{adults} adult(s)"));
        }
        if let Some(children) = self.children.filter(|&children| children > 0) {
            parts.push(format!("{children} child(ren)"));
        }
        if let Some(ref property_type) = self.property_type {
            parts.push(property_type.clone());
        }
        for (label, value) in [
            ("bedrooms", self.filters.min_bedrooms),
            ("beds", self.filters.min_beds),
            ("bathrooms", self.filters.min_bathrooms),
        ] {
            if let Some(value) = value {
                parts.push(format!("{value}+ {label}"));
            }
        }
        if !self.filters.amenities.is_empty() {
            parts.push(format!("amenities: {}", self.filters.amenities.join(", ")));
        }
        if self.filters.guest_favorite {
            parts.push("Guest Favorite".to_string());
        }
        if self.filters.instant_book {
            parts.push("Instant Book".to_string());
        }
        parts
    }
}

/// With stay dates, read the listing's own price for that stay from the
/// booking panel and use it as its nightly price (an undated detail has
/// none). A failed or unbookable quote leaves the detail untouched.
async fn fill_dated_price(
    client: &dyn AirbnbClient,
    detail: &mut ListingDetail,
    stay: &StayFilter,
) {
    let Some(request) = stay.quote_request(&detail.id) else {
        return;
    };
    match client.get_price_quote(&request).await {
        Ok(quote) => {
            // The basis search results use, so the listing compares like
            // for like with its comparables; the fee-inclusive average only
            // when the panel gives no other figure.
            if let Some(nightly) = quote
                .listed_nightly
                .or(quote.nightly)
                .filter(|nightly| *nightly > 0.0)
            {
                detail.price_per_night = nightly;
                detail.currency = quote.currency;
            }
        }
        Err(e) => tracing::debug!(
            error = %e.to_string().escape_debug(),
            "no dated price for the listing; the analysis runs without it"
        ),
    }
}

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
            )?;
            let max = usize::try_from(max).unwrap_or(usize::MAX);
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
        pages_fetched = pages_fetched.saturating_add(1);

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
        pages_fetched = pages_fetched.saturating_add(1);
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
    stay: &StayFilter,
) -> Result<RevenueEstimate> {
    if let Some(id) = id {
        validate_listing_id(id)?;
    }
    check_location_override(location.as_deref())?;
    stay.validate()?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    if let Some(id) = id {
        let mut detail = client.get_listing_detail(id).await?;
        fill_dated_price(client.as_ref(), &mut detail, stay).await;
        let location = location.unwrap_or_else(|| detail.location.clone());
        let calendar = client.get_price_calendar(id, months).await?;
        let sp = stay.search(location.clone());
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
        let sp = stay.search(location.clone());
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
    stay: &StayFilter,
) -> Result<AmenityAnalysis> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    stay.validate()?;
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = stay.search(location.clone());
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
    stay: &StayFilter,
) -> Result<CompetitivePositioning> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    stay.validate()?;
    let mut detail = client.get_listing_detail(id).await?;
    fill_dated_price(client.as_ref(), &mut detail, stay).await;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = stay.search(location);
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
    stay: &StayFilter,
) -> Result<PricingRecommendation> {
    validate_listing_id(id)?;
    check_location_override(location.as_deref())?;
    stay.validate()?;
    let months = limits::in_range("months", months, limits::MONTHS_MIN, limits::MONTHS_MAX)?;
    let mut detail = client.get_listing_detail(id).await?;
    fill_dated_price(client.as_ref(), &mut detail, stay).await;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = stay.search(location);
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

/// Price quotes: the requested stay, then the same stay one week later,
/// `weeks` stays in all (1–12). Every stay is validated before the first
/// request; an unbookable stay is a quote, an upstream failure is an error.
pub async fn run_price_quote(
    client: Arc<dyn AirbnbClient>,
    request: &QuoteRequest,
    weeks: u32,
) -> Result<QuoteSeries> {
    let weeks = limits::in_range(
        "weeks",
        weeks,
        limits::QUOTE_WEEKS_MIN,
        limits::QUOTE_WEEKS_MAX,
    )?;
    let mut stays = Vec::new();
    for offset in 0..weeks {
        let stay = request
            .shifted_by_weeks(offset)
            .ok_or_else(|| AirbnbError::InvalidParams {
                reason: format!("the stay cannot be moved {offset} week(s) later"),
            })?;
        stay.validate()?;
        stays.push(stay);
    }
    let mut quotes = Vec::with_capacity(stays.len());
    for stay in &stays {
        quotes.push(client.get_price_quote(stay).await?);
    }
    Ok(QuoteSeries {
        listing_id: request.listing_id.clone(),
        quotes,
    })
}

/// A rank measure: which listing, in which search, how deep to look.
#[derive(Debug, Clone)]
pub struct RankRequest {
    /// The listing to locate.
    pub listing_id: String,
    /// Location to search; the listing's own location when omitted.
    pub location: Option<String>,
    /// Stay and filters of the search.
    pub stay: StayFilter,
    /// Search pages to scan at most (1–15).
    pub max_pages: u32,
}

/// Search rank: page through the search in Airbnb's order until the listing
/// shows up or `max_pages` pages are scanned. Results are unique by id, so
/// a listing repeated on two pages counts once.
pub async fn run_listing_rank(
    client: Arc<dyn AirbnbClient>,
    request: &RankRequest,
) -> Result<ListingRank> {
    validate_listing_id(&request.listing_id)?;
    check_location_override(request.location.as_deref())?;
    request.stay.validate()?;
    let max_pages = limits::in_range(
        "max_pages",
        request.max_pages,
        limits::RANK_PAGES_MIN,
        limits::RANK_PAGES_MAX,
    )?;
    let location = match request.location {
        Some(ref location) => location.clone(),
        None => {
            client
                .get_listing_detail(&request.listing_id)
                .await?
                .location
        }
    };
    let base = request.stay.search(location.clone());
    base.validate()?;

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_cursors: HashSet<String> = HashSet::new();
    let mut listings: Vec<Listing> = Vec::new();
    let mut pages_fetched: u32 = 0;
    let mut cursor: Option<String> = None;
    let mut exhausted = false;
    let mut found_on_page: Option<u32> = None;
    for _ in 0..max_pages {
        let params = SearchParams {
            cursor: cursor.clone(),
            ..base.clone()
        };
        let page = client.search_listings(&params).await?;
        pages_fetched = pages_fetched.saturating_add(1);
        for listing in page.listings {
            if seen_ids.insert(listing.id.clone()) {
                listings.push(listing);
            }
        }
        if seen_ids.contains(&request.listing_id) {
            found_on_page = Some(pages_fetched);
            break;
        }
        // A page that only repeats earlier results is not the end: Airbnb
        // says so by giving no further cursor.
        match page.next_cursor {
            Some(next) if seen_cursors.insert(next.clone()) => cursor = Some(next),
            _ => {
                exhausted = true;
                break;
            }
        }
    }

    let mut search = vec![location];
    search.extend(request.stay.describe());
    Ok(compute_listing_rank(&RankScan {
        listing_id: &request.listing_id,
        search: search.join(" | "),
        listings: &listings,
        pages_fetched,
        found_on_page,
        exhausted,
    }))
}

/// Location searched by the status checks when none is given.
pub const STATUS_DEFAULT_LOCATION: &str = "Paris, France";

/// Days between today and the check-in of the status checks' sample stay.
const STATUS_STAY_LEAD_DAYS: u64 = 60;
/// Nights of the status checks' sample stay.
const STATUS_STAY_NIGHTS: u64 = 3;

fn failed(check: &str, err: &AirbnbError) -> Probe {
    Probe::new(check, ProbeState::Failed, err.to_string())
}

/// A check that could not run because the search gave no listing:
/// `search_failed` tells a failed search from an empty one.
fn skipped(check: &str, search_failed: bool) -> Probe {
    Probe::new(
        check,
        ProbeState::Skipped,
        if search_failed {
            "no listing to check (the search failed)"
        } else {
            "no listing to check (the search returned none)"
        },
    )
}

/// `OK` with `facts`, or `DEGRADED` naming what is `missing`.
fn graded(check: &str, facts: String, missing: &[&str]) -> Probe {
    if missing.is_empty() {
        Probe::new(check, ProbeState::Ok, facts)
    } else {
        Probe::new(
            check,
            ProbeState::Degraded,
            format!("{facts}; missing: {}", missing.join(", ")),
        )
    }
}

/// Run the six checks of one listing: details, reviews, calendar, host
/// profile and price quote, after the search that found it.
///
/// `has_reviews` says the search showed reviews for the listing: an empty
/// first page is then missing data, not a listing without reviews.
async fn probe_listing(
    client: &dyn AirbnbClient,
    id: &str,
    quote: &QuoteRequest,
    has_reviews: bool,
) -> Vec<Probe> {
    let mut probes = Vec::new();
    probes.push(match client.get_listing_detail(id).await {
        Ok(detail) => {
            let mut missing = Vec::new();
            if detail.description.is_empty() {
                missing.push("description");
            }
            if detail.amenities.is_empty() {
                missing.push("amenities");
            }
            if detail.photos.is_empty() {
                missing.push("photos");
            }
            if detail.host_name.is_none() {
                missing.push("host");
            }
            graded(
                "listing details",
                format!(
                    "{} amenities, {} photos, description of {} characters",
                    detail.amenities.len(),
                    detail.photos.len(),
                    detail.description.chars().count()
                ),
                &missing,
            )
        }
        Err(e) => failed("listing details", &e),
    });
    probes.push(match client.get_reviews(id, None).await {
        Ok(page) if has_reviews && page.reviews.is_empty() => Probe::new(
            "reviews",
            ProbeState::Degraded,
            "no review returned for a listing the search shows with reviews",
        ),
        Ok(page) => Probe::new(
            "reviews",
            ProbeState::Ok,
            match page.total_reviews {
                Some(total) => {
                    format!("{} reviews on the page, {total} in all", page.reviews.len())
                }
                None => format!("{} reviews on the page", page.reviews.len()),
            },
        ),
        Err(e) => failed("reviews", &e),
    });
    probes.push(match client.get_price_calendar(id, 1).await {
        Ok(calendar) if calendar.days.is_empty() => {
            Probe::new("calendar", ProbeState::Degraded, "no day returned")
        }
        Ok(calendar) => Probe::new(
            "calendar",
            ProbeState::Ok,
            format!("{} days", calendar.days.len()),
        ),
        Err(e) => failed("calendar", &e),
    });
    probes.push(match client.get_host_profile(id).await {
        Ok(host) => Probe::new(
            "host profile",
            ProbeState::Ok,
            format!("host {}", host.name),
        ),
        // A business listing has no host profile: the source answered.
        Err(e @ AirbnbError::HostProfileUnavailable { .. }) => {
            Probe::new("host profile", ProbeState::Ok, e.to_string())
        }
        Err(e) => failed("host profile", &e),
    });
    probes.push(match client.get_price_quote(quote).await {
        Ok(quoted) => Probe::new(
            "price quote",
            ProbeState::Ok,
            match (
                quoted.available,
                &quoted.total_display,
                &quoted.unavailability_message,
            ) {
                (true, Some(total), _) => format!("bookable, total {total}"),
                (true, None, _) => "bookable".to_string(),
                (false, _, Some(reason)) => format!("not bookable: {reason}"),
                (false, _, None) => "not bookable".to_string(),
            },
        ),
        Err(e) => failed("price quote", &e),
    });
    probes
}

/// Source status: one dated search of `location` (default
/// [`STATUS_DEFAULT_LOCATION`]), then every other operation on a listing it
/// returned. A failed check is part of the report, never an error: only an
/// invalid location is.
pub async fn run_status(
    client: Arc<dyn AirbnbClient>,
    location: Option<String>,
) -> Result<StatusReport> {
    let location = location.unwrap_or_else(|| STATUS_DEFAULT_LOCATION.to_string());
    limits::check_location(&location)?;
    let today = chrono::Utc::now().date_naive();
    let checkin = today
        .checked_add_days(chrono::Days::new(STATUS_STAY_LEAD_DAYS))
        .unwrap_or(today);
    let checkout = checkin
        .checked_add_days(chrono::Days::new(STATUS_STAY_NIGHTS))
        .unwrap_or(checkin);
    let checkin = checkin.format("%Y-%m-%d").to_string();
    let checkout = checkout.format("%Y-%m-%d").to_string();
    let search = SearchParams {
        location: location.clone(),
        checkin: Some(checkin.clone()),
        checkout: Some(checkout.clone()),
        adults: Some(2),
        ..SearchParams::default()
    };

    // A cached answer says nothing about the source: every check below
    // must reach Airbnb.
    client.clear_cache();
    let mut probes = Vec::new();
    let found = client.search_listings(&search).await;
    let sample_listing = found.as_ref().ok().and_then(|result| {
        result
            .listings
            .iter()
            .find(|listing| listing.review_count > 0)
            .or_else(|| result.listings.first())
    });
    let has_reviews = sample_listing.is_some_and(|listing| listing.review_count > 0);
    let sample_id = sample_listing.map(|listing| listing.id.clone());
    probes.push(match &found {
        Ok(result) => {
            let priced = result
                .listings
                .iter()
                .filter(|listing| listing.known_price().is_some())
                .count();
            let facts = format!("{} listings, {priced} with a price", result.listings.len());
            let mut missing = Vec::new();
            if result.listings.is_empty() {
                missing.push("listings");
            } else if priced == 0 {
                missing.push("prices");
            }
            graded("search", facts, &missing)
        }
        Err(e) => failed("search", e),
    });

    let sample = if let Some(ref id) = sample_id {
        let quote = QuoteRequest {
            listing_id: id.clone(),
            checkin: checkin.clone(),
            checkout: checkout.clone(),
            adults: 2,
        };
        probes.extend(probe_listing(client.as_ref(), id, &quote, has_reviews).await);
        format!("listing {id}, from a search of {location} for {checkin} to {checkout}, 2 adults")
    } else {
        for check in [
            "listing details",
            "reviews",
            "calendar",
            "host profile",
            "price quote",
        ] {
            probes.push(skipped(check, found.is_err()));
        }
        format!("search of {location} for {checkin} to {checkout}, 2 adults")
    };

    Ok(StatusReport {
        sample,
        probes,
        degradations: client.degradations(),
    })
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
        let err = run_amenity_analysis(client, "42", None, &StayFilter::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    #[tokio::test]
    async fn amenity_analysis_search_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited)));
        let err = run_amenity_analysis(client, "42", None, &StayFilter::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn revenue_estimate_calendar_failure_propagates() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_calendar(|_, _| Err(AirbnbError::RateLimited)));
        let err = run_revenue_estimate(
            client,
            Some("42"),
            Some("Paris".into()),
            12,
            &StayFilter::default(),
        )
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
        let estimate = run_revenue_estimate(client, Some("42"), None, 12, &StayFilter::default())
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
        let err = run_revenue_estimate(
            client,
            Some("42"),
            Some("Paris".into()),
            12,
            &StayFilter::default(),
        )
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
        let estimate = run_revenue_estimate(
            client,
            Some("42"),
            Some("Paris".into()),
            12,
            &StayFilter::default(),
        )
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
        let err = run_optimal_pricing(client, "42", None, 12, &StayFilter::default())
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
        let err = run_optimal_pricing(
            client,
            "42",
            Some("Paris".into()),
            12,
            &StayFilter::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn optimal_pricing_neighborhood_failure_is_not_fatal_with_a_listing_price() {
        let client: Arc<dyn AirbnbClient> =
            Arc::new(MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited)));
        let recommendation = run_optimal_pricing(
            client,
            "42",
            Some("Paris".into()),
            12,
            &StayFilter::default(),
        )
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
        let err = run_competitive_positioning(client, "42", None, &StayFilter::default())
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
        let estimate = run_revenue_estimate(
            Arc::new(mock),
            None,
            Some("Rome, Italy".into()),
            12,
            &StayFilter::default(),
        )
        .await
        .unwrap();
        assert!(estimate.listing_id.is_none());
        assert_eq!(estimate.location, "Rome, Italy");
    }

    #[tokio::test]
    async fn revenue_requires_an_id_or_a_location() {
        let err = run_revenue_estimate(
            Arc::new(MockAirbnbClient::new()),
            None,
            None,
            12,
            &StayFilter::default(),
        )
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
            run_amenity_analysis(Arc::clone(&client), "0042", None, &StayFilter::default())
                .await
                .is_err()
        );
        assert!(
            run_host_portfolio(Arc::clone(&client), "0042")
                .await
                .is_err()
        );
        assert!(
            run_competitive_positioning(Arc::clone(&client), "abc", None, &StayFilter::default())
                .await
                .is_err()
        );
        assert!(
            run_optimal_pricing(Arc::clone(&client), "abc", None, 12, &StayFilter::default())
                .await
                .is_err()
        );
        assert!(
            run_optimal_pricing(Arc::clone(&client), "42", None, 13, &StayFilter::default())
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
                &StayFilter::default(),
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
            let result = run_amenity_analysis(
                counting_client(&calls),
                "42",
                Some(location.clone()),
                &StayFilter::default(),
            )
            .await;
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
            let result = run_competitive_positioning(
                counting_client(&calls),
                "42",
                Some(location.clone()),
                &StayFilter::default(),
            )
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
            let result = run_optimal_pricing(
                counting_client(&calls),
                "42",
                Some(location.clone()),
                12,
                &StayFilter::default(),
            )
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
        run_optimal_pricing(
            Arc::clone(&client),
            "42",
            Some("Lyon, France".into()),
            12,
            &StayFilter::default(),
        )
        .await
        .unwrap();
        run_revenue_estimate(
            client,
            Some("42"),
            Some("Lyon, France".into()),
            12,
            &StayFilter::default(),
        )
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

#[cfg(test)]
mod stay_tests {
    use std::sync::Mutex;

    use super::*;
    use crate::domain::listing::SearchResult;
    use crate::test_helpers::{
        MockAirbnbClient, make_listing, make_listing_detail, make_price_quote, make_search_result,
    };

    /// A stay `lead` days ahead, 7 nights.
    fn stay_in(lead: u64) -> (String, String) {
        let checkin = chrono::Utc::now().date_naive() + chrono::Days::new(lead);
        let checkout = checkin + chrono::Days::new(7);
        (checkin.to_string(), checkout.to_string())
    }

    fn dated(lead: u64) -> StayFilter {
        let (checkin, checkout) = stay_in(lead);
        StayFilter {
            checkin: Some(checkin),
            checkout: Some(checkout),
            adults: Some(4),
            ..StayFilter::default()
        }
    }

    fn quote_request(lead: u64) -> QuoteRequest {
        let (checkin, checkout) = stay_in(lead);
        QuoteRequest {
            listing_id: "42".into(),
            checkin,
            checkout,
            adults: 2,
        }
    }

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

    #[test]
    fn a_stay_filter_becomes_a_search_a_quote_and_words() {
        let stay = StayFilter {
            checkin: Some("2027-07-10".into()),
            checkout: Some("2027-07-17".into()),
            adults: Some(4),
            children: Some(1),
            property_type: Some("Entire home".into()),
            filters: SearchFilters {
                min_bedrooms: Some(2),
                amenities: vec!["pool".into()],
                guest_favorite: true,
                instant_book: true,
                ..SearchFilters::default()
            },
        };
        let search = stay.search("Porto-Vecchio".into());
        assert_eq!(search.location, "Porto-Vecchio");
        assert_eq!(search.checkin.as_deref(), Some("2027-07-10"));
        assert_eq!(search.adults, Some(4));
        assert_eq!(search.children, Some(1));
        assert_eq!(search.property_type.as_deref(), Some("Entire home"));
        assert_eq!(search.filters, stay.filters);
        assert_eq!(search.cursor, None);

        let quote = stay.quote_request("42").unwrap();
        assert_eq!(
            (quote.listing_id.as_str(), quote.adults),
            ("42", 4),
            "{quote:?}"
        );
        assert_eq!(quote.checkout, "2027-07-17");
        assert_eq!(
            stay.describe(),
            [
                "2027-07-10 to 2027-07-17",
                "4 adult(s)",
                "1 child(ren)",
                "Entire home",
                "2+ bedrooms",
                "amenities: pool",
                "Guest Favorite",
                "Instant Book",
            ]
        );
    }

    #[test]
    fn an_undated_stay_has_no_quote_and_a_quote_needs_one_adult() {
        assert_eq!(StayFilter::default().quote_request("42"), None);
        assert_eq!(StayFilter::default().describe(), Vec::<String>::new());
        let no_adults = StayFilter {
            adults: Some(0),
            ..dated(30)
        };
        assert_eq!(no_adults.quote_request("42").unwrap().adults, 1);
        let unset = StayFilter {
            adults: None,
            ..dated(30)
        };
        assert_eq!(unset.quote_request("42").unwrap().adults, 1);
    }

    #[test]
    fn a_stay_filter_is_checked_with_the_search_rules() {
        assert!(StayFilter::default().validate().is_ok());
        assert!(dated(30).validate().is_ok());
        let half = StayFilter {
            checkout: None,
            ..dated(30)
        };
        assert!(half.validate().is_err(), "checkin without checkout");
        let unknown_amenity = StayFilter {
            filters: SearchFilters {
                amenities: vec!["jacuzzi on the moon".into()],
                ..SearchFilters::default()
            },
            ..StayFilter::default()
        };
        assert!(unknown_amenity.validate().is_err());
    }

    #[tokio::test]
    async fn price_quotes_cover_consecutive_weeks_in_order() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = Arc::clone(&seen);
        let mock = MockAirbnbClient::new().with_quote(move |request| {
            let mut log = log.lock().unwrap();
            log.push(request.checkin.clone());
            // The second week cannot be booked.
            let nightly = (log.len() != 2).then_some(150.0);
            Ok(make_price_quote(request, nightly))
        });
        let request = quote_request(30);
        let series = run_price_quote(Arc::new(mock), &request, 3).await.unwrap();
        assert_eq!(series.listing_id, "42");
        assert_eq!(series.quotes.len(), 3);
        let expected: Vec<String> = (0..3)
            .map(|week| request.shifted_by_weeks(week).unwrap().checkin)
            .collect();
        assert_eq!(*seen.lock().unwrap(), expected);
        let available: Vec<bool> = series.quotes.iter().map(|q| q.available).collect();
        assert_eq!(available, [true, false, true]);
    }

    #[tokio::test]
    async fn price_quotes_validate_every_week_before_the_first_request() {
        let calls = Arc::new(Mutex::new(0_u32));
        let counter = Arc::clone(&calls);
        let mock = Arc::new(MockAirbnbClient::new().with_quote(move |request| {
            *counter.lock().unwrap() += 1;
            Ok(make_price_quote(request, Some(100.0)))
        }));
        // 13 weeks is out of range.
        let client: Arc<dyn AirbnbClient> = Arc::clone(&mock) as Arc<dyn AirbnbClient>;
        let err = run_price_quote(Arc::clone(&client), &quote_request(30), 13)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("weeks"), "{err}");
        // The first stay is fine; the 12th starts more than 730 days ahead.
        let err = run_price_quote(Arc::clone(&client), &quote_request(700), 12)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::InvalidParams { .. }), "{err}");
        // An invalid listing id.
        let bad_id = QuoteRequest {
            listing_id: "abc".into(),
            ..quote_request(30)
        };
        assert!(run_price_quote(client, &bad_id, 1).await.is_err());
        assert_eq!(*calls.lock().unwrap(), 0, "nothing was fetched");
    }

    #[tokio::test]
    async fn a_failed_quote_fails_the_series() {
        let mock = MockAirbnbClient::new().with_quote(|_| Err(AirbnbError::RateLimited));
        let err = run_price_quote(Arc::new(mock), &quote_request(30), 2)
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    fn rank_request(id: &str, location: Option<&str>, max_pages: u32) -> RankRequest {
        RankRequest {
            listing_id: id.into(),
            location: location.map(String::from),
            stay: StayFilter::default(),
            max_pages,
        }
    }

    #[tokio::test]
    async fn rank_stops_at_the_page_that_holds_the_listing() {
        let cursors = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let log = Arc::clone(&cursors);
        let mock = MockAirbnbClient::new().with_search(move |params| {
            log.lock().unwrap().push(params.cursor.clone());
            Ok(match params.cursor.as_deref() {
                None => page(&["1", "2", "3"], Some("c2")),
                Some("c2") => page(&["3", "4", "42"], Some("c3")),
                Some(_) => page(&["9"], None),
            })
        });
        let rank = run_listing_rank(Arc::new(mock), &rank_request("42", Some("Paris"), 5))
            .await
            .unwrap();
        assert_eq!(rank.position, Some(5), "listing 3 is counted once");
        assert_eq!(rank.page, Some(2), "found on the second page fetched");
        assert_eq!(rank.scanned, 5);
        assert_eq!(rank.pages_fetched, 2);
        assert!(!rank.exhausted);
        assert_eq!(rank.search, "Paris");
        assert_eq!(
            *cursors.lock().unwrap(),
            [None, Some("c2".to_string())],
            "page 3 is never fetched"
        );
    }

    #[tokio::test]
    async fn rank_reports_an_absent_listing_and_whether_the_search_was_exhausted() {
        // Airbnb has two pages and the listing is on neither.
        let two_pages = || {
            MockAirbnbClient::new().with_search(|params| {
                Ok(match params.cursor.as_deref() {
                    None => page(&["1", "2"], Some("c2")),
                    Some(_) => page(&["3"], None),
                })
            })
        };
        let all = run_listing_rank(Arc::new(two_pages()), &rank_request("42", Some("Paris"), 5))
            .await
            .unwrap();
        assert_eq!(all.position, None);
        assert_eq!((all.scanned, all.pages_fetched), (3, 2));
        assert!(all.exhausted);

        // The page budget ends the scan first.
        let capped = run_listing_rank(Arc::new(two_pages()), &rank_request("42", Some("Paris"), 1))
            .await
            .unwrap();
        assert_eq!((capped.scanned, capped.pages_fetched), (2, 1));
        assert!(!capped.exhausted);

        // A page that only repeats earlier results is not the end when
        // Airbnb gives a further cursor: the listing on page 3 is found.
        let repeating = MockAirbnbClient::new().with_search(|params| {
            Ok(match params.cursor.as_deref() {
                None => page(&["1", "2"], Some("c2")),
                Some("c2") => page(&["2", "1"], Some("c3")),
                Some(_) => page(&["42"], None),
            })
        });
        let found = run_listing_rank(Arc::new(repeating), &rank_request("42", Some("Paris"), 5))
            .await
            .unwrap();
        assert_eq!(found.position, Some(3));
        assert_eq!(found.page, Some(3), "the page comes from the scan");
        assert_eq!(found.pages_fetched, 3);

        // A cursor that loops ends the scan.
        let looping = MockAirbnbClient::new().with_search(|_| Ok(page(&["1", "2"], Some("same"))));
        let looped = run_listing_rank(Arc::new(looping), &rank_request("42", Some("Paris"), 15))
            .await
            .unwrap();
        assert_eq!(looped.pages_fetched, 2);
        assert!(looped.exhausted);
    }

    #[tokio::test]
    async fn rank_uses_the_listing_location_and_the_stay_filters() {
        let searched = Arc::new(Mutex::new(Vec::<SearchParams>::new()));
        let log = Arc::clone(&searched);
        let mock = MockAirbnbClient::new()
            .with_detail(|id| {
                let mut detail = make_listing_detail(id);
                detail.location = "Porto-Vecchio, Corsica".into();
                Ok(detail)
            })
            .with_search(move |params| {
                log.lock().unwrap().push(params.clone());
                Ok(page(&["42"], None))
            });
        let request = RankRequest {
            listing_id: "42".into(),
            location: None,
            stay: StayFilter {
                property_type: Some("Entire home".into()),
                filters: SearchFilters {
                    min_bedrooms: Some(2),
                    amenities: vec!["pool".into()],
                    ..SearchFilters::default()
                },
                ..dated(30)
            },
            max_pages: 3,
        };
        let rank = run_listing_rank(Arc::new(mock), &request).await.unwrap();
        assert_eq!(rank.position, Some(1));
        let searched = searched.lock().unwrap();
        let params = searched.first().unwrap();
        assert_eq!(params.location, "Porto-Vecchio, Corsica");
        assert_eq!(params.adults, Some(4));
        assert_eq!(params.filters.min_bedrooms, Some(2));
        assert_eq!(params.filters.amenities, ["pool"]);
        assert!(
            rank.search.starts_with("Porto-Vecchio, Corsica | "),
            "{}",
            rank.search
        );
        assert!(rank.search.contains("2+ bedrooms"), "{}", rank.search);
        assert!(rank.search.ends_with("amenities: pool"), "{}", rank.search);
    }

    #[tokio::test]
    async fn rank_rejects_bad_input_before_fetching() {
        let calls = Arc::new(Mutex::new(0_u32));
        let searches = Arc::clone(&calls);
        let details = Arc::clone(&calls);
        let mock = Arc::new(
            MockAirbnbClient::new()
                .with_search(move |_| {
                    *searches.lock().unwrap() += 1;
                    Ok(make_search_result(vec![]))
                })
                .with_detail(move |id| {
                    *details.lock().unwrap() += 1;
                    Ok(make_listing_detail(id))
                }),
        );
        let client: Arc<dyn AirbnbClient> = mock;
        let half_dated = RankRequest {
            stay: StayFilter {
                checkout: None,
                ..dated(30)
            },
            ..rank_request("42", None, 5)
        };
        for request in [
            rank_request("abc", Some("Paris"), 5),
            rank_request("42", Some("   "), 5),
            rank_request("42", Some("Paris"), 0),
            rank_request("42", Some("Paris"), 16),
            half_dated,
        ] {
            let err = run_listing_rank(Arc::clone(&client), &request)
                .await
                .unwrap_err();
            assert!(
                matches!(err, AirbnbError::InvalidParams { .. }),
                "{request:?}: {err}"
            );
        }
        assert_eq!(*calls.lock().unwrap(), 0, "nothing was fetched");
    }

    #[tokio::test]
    async fn status_checks_every_operation_on_a_listing_of_the_search() {
        let quoted = Arc::new(Mutex::new(Vec::<QuoteRequest>::new()));
        let log = Arc::clone(&quoted);
        let mock = MockAirbnbClient::new()
            .with_search(|params| {
                assert_eq!(params.location, STATUS_DEFAULT_LOCATION);
                assert_eq!(params.adults, Some(2));
                assert!(params.checkin.is_some() && params.checkout.is_some());
                let mut unreviewed = make_listing("7", "New flat", 90.0);
                unreviewed.review_count = 0;
                let mut reviewed = make_listing("8", "Known flat", 120.0);
                reviewed.review_count = 12;
                Ok(make_search_result(vec![unreviewed, reviewed]))
            })
            .with_quote(move |request| {
                log.lock().unwrap().push(request.clone());
                Ok(make_price_quote(request, Some(100.0)))
            });
        let report = run_status(Arc::new(mock), None).await.unwrap();
        let checks: Vec<&str> = report.probes.iter().map(|p| p.check.as_str()).collect();
        assert_eq!(
            checks,
            [
                "search",
                "listing details",
                "reviews",
                "calendar",
                "host profile",
                "price quote"
            ]
        );
        assert!(
            report
                .sample
                .starts_with("listing 8, from a search of Paris, France for "),
            "the sample is the first listing with reviews: {}",
            report.sample
        );
        let quoted = quoted.lock().unwrap();
        let quote = quoted.first().unwrap();
        assert_eq!((quote.listing_id.as_str(), quote.adults), ("8", 2));
        assert_eq!(quote.validate().unwrap(), 3, "a valid 3-night stay");
        let search = report.probes.first().unwrap();
        assert_eq!(search.state, ProbeState::Ok);
        assert_eq!(search.detail, "2 listings, 2 with a price");
        let quote_probe = report.probes.last().unwrap();
        assert_eq!(quote_probe.detail, "bookable, total $700");
        assert_eq!(report.degradations, Vec::new());
        // The search shows 12 reviews and the page came back empty.
        let reviews = report.probes.get(2).unwrap();
        assert_eq!(reviews.state, ProbeState::Degraded);
        assert!(
            reviews.detail.contains("no review returned"),
            "{}",
            reviews.detail
        );
    }

    #[tokio::test]
    async fn status_accepts_an_empty_reviews_page_for_a_listing_without_reviews() {
        let unreviewed = || {
            MockAirbnbClient::new().with_search(|_| {
                let mut listing = make_listing("7", "New flat", 90.0);
                listing.review_count = 0;
                Ok(make_search_result(vec![listing]))
            })
        };
        let report = run_status(Arc::new(unreviewed()), None).await.unwrap();
        assert_eq!(report.probes.get(2).unwrap().state, ProbeState::Ok);

        // With reviews on the page, a reviewed listing passes too.
        let reviewed = MockAirbnbClient::new()
            .with_search(|_| Ok(make_search_result(vec![make_listing("8", "Flat", 120.0)])))
            .with_reviews(|id, _| {
                Ok(crate::test_helpers::make_reviews_page(
                    id,
                    vec![crate::test_helpers::make_review("Guest A", "Great stay")],
                ))
            });
        let report = run_status(Arc::new(reviewed), None).await.unwrap();
        let reviews = report.probes.get(2).unwrap();
        assert_eq!(reviews.state, ProbeState::Ok, "{}", reviews.detail);
    }

    #[tokio::test]
    async fn status_reports_failures_and_missing_data_without_failing() {
        let mock = MockAirbnbClient::new()
            .with_search(|_| Ok(make_search_result(vec![make_listing("7", "Flat", 0.0)])))
            .with_detail(|id| {
                let mut detail = make_listing_detail(id);
                detail.description = String::new();
                detail.amenities = vec![];
                Ok(detail)
            })
            .with_reviews(|_, _| {
                Err(AirbnbError::UpstreamSchema {
                    operation: "StaysPdpReviewsQuery".into(),
                    detail: "HTTP 400".into(),
                })
            })
            .with_calendar(|id, _| Ok(crate::test_helpers::make_price_calendar(id, vec![])))
            .with_host_profile(|id| {
                Err(AirbnbError::HostProfileUnavailable {
                    listing_id: id.to_string(),
                    reason: "business host".into(),
                })
            })
            .with_quote(|request| Ok(make_price_quote(request, None)));
        let report = run_status(Arc::new(mock), Some("Lyon, France".into()))
            .await
            .unwrap();
        let states: Vec<(&str, ProbeState)> = report
            .probes
            .iter()
            .map(|p| (p.check.as_str(), p.state))
            .collect();
        assert_eq!(
            states,
            [
                ("search", ProbeState::Degraded),
                ("listing details", ProbeState::Degraded),
                ("reviews", ProbeState::Failed),
                ("calendar", ProbeState::Degraded),
                ("host profile", ProbeState::Ok),
                ("price quote", ProbeState::Ok),
            ]
        );
        let text = report.to_string();
        assert!(
            text.contains("1 listings, 0 with a price; missing: prices"),
            "{text}"
        );
        assert!(text.contains("missing: description, amenities"), "{text}");
        assert!(text.contains("StaysPdpReviewsQuery"), "{text}");
        assert!(
            text.contains("not bookable: Those dates are not available"),
            "{text}"
        );
        assert!(text.contains("from a search of Lyon, France"), "{text}");
        assert!(!report.healthy());
    }

    #[tokio::test]
    async fn status_skips_the_listing_checks_when_the_search_fails_or_is_empty() {
        let failing = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let clears = failing.cache_clears();
        let report = run_status(Arc::new(failing), None).await.unwrap();
        assert_eq!(
            clears.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the checks must reach Airbnb, not the cache"
        );
        assert_eq!(report.probes.len(), 6);
        assert_eq!(report.probes.first().unwrap().state, ProbeState::Failed);
        assert!(report.probes.iter().skip(1).all(|probe| {
            probe.state == ProbeState::Skipped && probe.detail.contains("the search failed")
        }));
        assert!(report.sample.starts_with("search of Paris, France for "));

        let empty = MockAirbnbClient::new();
        let report = run_status(Arc::new(empty), None).await.unwrap();
        assert!(report.probes.iter().skip(1).all(|probe| {
            probe.state == ProbeState::Skipped && probe.detail.contains("the search returned none")
        }));
        let search = report.probes.first().unwrap();
        assert_eq!(search.state, ProbeState::Degraded);
        assert!(
            search.detail.contains("missing: listings"),
            "{}",
            search.detail
        );

        assert!(
            run_status(Arc::new(MockAirbnbClient::new()), Some("   ".into()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn status_lists_the_fallbacks_the_client_reports() {
        let mock = MockAirbnbClient::new()
            .with_search(|_| Ok(make_search_result(vec![make_listing("7", "Flat", 100.0)])))
            .with_degradation(
                "get_listing_detail",
                "Airbnb API changed for StaysPdpSections: HTTP 400",
            );
        let report = run_status(Arc::new(mock), None).await.unwrap();
        assert_eq!(report.degradations.len(), 1);
        assert!(!report.healthy());
        assert!(
            report
                .to_string()
                .contains("  - listing details: Airbnb API changed for StaysPdpSections: HTTP 400")
        );
    }

    #[tokio::test]
    async fn dated_analyses_price_the_listing_with_its_quote_and_filter_the_comparables() {
        let searched = Arc::new(Mutex::new(Vec::<SearchParams>::new()));
        let log = Arc::clone(&searched);
        let build = move || {
            let log = Arc::clone(&log);
            MockAirbnbClient::new()
                .with_detail(|id| {
                    let mut detail = make_listing_detail(id);
                    detail.price_per_night = 0.0;
                    Ok(detail)
                })
                .with_neighborhood(move |params| {
                    log.lock().unwrap().push(params.clone());
                    Ok(crate::test_helpers::make_neighborhood_stats(
                        &params.location,
                    ))
                })
                .with_quote(|request| Ok(make_price_quote(request, Some(321.0))))
        };
        let stay = StayFilter {
            property_type: Some("Entire home".into()),
            filters: SearchFilters {
                min_bedrooms: Some(2),
                ..SearchFilters::default()
            },
            ..dated(30)
        };
        let pricing = run_optimal_pricing(Arc::new(build()), "42", None, 3, &stay)
            .await
            .unwrap();
        assert_eq!(
            pricing.current_price,
            Some(321.0),
            "the undated detail has no price; the quote gives it"
        );
        // Without dates there is no quote: the same listing has no price at all.
        let undated = run_optimal_pricing(Arc::new(build()), "42", None, 3, &StayFilter::default())
            .await
            .unwrap_err();
        assert!(
            matches!(undated, AirbnbError::InsufficientData { .. }),
            "{undated}"
        );

        let searched = searched.lock().unwrap();
        let first = searched.first().unwrap();
        assert_eq!(first.checkin, stay.checkin);
        assert_eq!(first.adults, Some(4));
        assert_eq!(first.property_type.as_deref(), Some("Entire home"));
        assert_eq!(first.filters.min_bedrooms, Some(2));
        let last = searched.last().unwrap();
        assert_eq!(last.checkin, None);
        assert!(last.filters.is_empty());
    }

    #[tokio::test]
    async fn the_dated_price_is_the_one_search_results_show_not_the_fee_inclusive_average() {
        let priced = |listed: Option<f64>, average: Option<f64>| {
            MockAirbnbClient::new()
                .with_detail(|id| {
                    let mut detail = make_listing_detail(id);
                    detail.price_per_night = 0.0;
                    Ok(detail)
                })
                .with_quote(move |request| {
                    let mut quote = make_price_quote(request, average);
                    quote.listed_nightly = listed;
                    Ok(quote)
                })
        };
        let current = |mock: MockAirbnbClient| async move {
            run_optimal_pricing(Arc::new(mock), "42", None, 3, &dated(30))
                .await
                .map(|pricing| pricing.current_price)
        };
        // Search results show 200; the total with fees averages 217.14.
        assert_eq!(
            current(priced(Some(200.0), Some(217.14))).await.unwrap(),
            Some(200.0)
        );
        // No breakdown line: the average is the only figure.
        assert_eq!(
            current(priced(None, Some(217.14))).await.unwrap(),
            Some(217.14)
        );
        // A zero is "unknown" on the wire: it never replaces the price.
        let zero = current(priced(Some(0.0), None)).await.unwrap_err();
        assert!(
            matches!(zero, AirbnbError::InsufficientData { .. }),
            "{zero}"
        );
    }

    #[tokio::test]
    async fn a_failed_or_unbookable_quote_leaves_the_analysis_running() {
        for quote in [
            MockAirbnbClient::new().with_quote(|_| Err(AirbnbError::RateLimited)),
            MockAirbnbClient::new().with_quote(|request| Ok(make_price_quote(request, None))),
        ] {
            let mock = quote.with_detail(|id| {
                let mut detail = make_listing_detail(id);
                detail.price_per_night = 80.0;
                Ok(detail)
            });
            let positioning = run_competitive_positioning(Arc::new(mock), "42", None, &dated(30))
                .await
                .unwrap();
            assert_eq!(positioning.listing_id, "42");
        }
        // An invalid stay is rejected before anything is fetched.
        let half = StayFilter {
            checkout: None,
            ..dated(30)
        };
        for result in [
            run_competitive_positioning(Arc::new(MockAirbnbClient::new()), "42", None, &half)
                .await
                .map(|_| ()),
            run_amenity_analysis(Arc::new(MockAirbnbClient::new()), "42", None, &half)
                .await
                .map(|_| ()),
            run_revenue_estimate(
                Arc::new(MockAirbnbClient::new()),
                Some("42"),
                None,
                3,
                &half,
            )
            .await
            .map(|_| ()),
            run_optimal_pricing(Arc::new(MockAirbnbClient::new()), "42", None, 3, &half)
                .await
                .map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(AirbnbError::InvalidParams { .. })),
                "{result:?}"
            );
        }
    }
}
