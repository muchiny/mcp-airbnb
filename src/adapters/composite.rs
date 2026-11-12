use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::{debug, warn};

use crate::adapters::shared::FetchCounter;
use crate::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::quote::{PriceQuote, QuoteRequest};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::domain::status::Degradation;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;

/// A client that tries the GraphQL API first and falls back to HTML scraping.
pub struct CompositeClient {
    graphql: Box<dyn AirbnbClient>,
    scraper: Box<dyn AirbnbClient>,
    /// Port methods currently served by the scraper, with the GraphQL error
    /// and the time of the fallback.
    degraded: Mutex<BTreeMap<&'static str, (String, Instant)>>,
    /// Network answers of the GraphQL client, when it reports them. Without
    /// it every `Ok` counts as an answer of the source.
    primary_fetches: Option<Arc<FetchCounter>>,
}

/// How long a fallback stays reported when the operation is not called
/// again: the search cache lifetime, after which nothing served is stale.
const DEGRADATION_TTL: Duration = Duration::from_mins(15);

/// Longest GraphQL error kept in a degradation, in characters.
const DEGRADATION_ERROR_CHARS: usize = 300;

impl CompositeClient {
    pub fn new(graphql: Box<dyn AirbnbClient>, scraper: Box<dyn AirbnbClient>) -> Self {
        Self {
            graphql,
            scraper,
            degraded: Mutex::new(BTreeMap::new()),
            primary_fetches: None,
        }
    }

    /// Tell cache hits from network answers of the primary source: a
    /// fallback is then forgotten only when GraphQL answered over the
    /// network, not when its cache did.
    #[must_use]
    pub fn with_primary_fetches(mut self, fetches: Arc<FetchCounter>) -> Self {
        self.primary_fetches = Some(fetches);
        self
    }

    /// Network answers of the primary source for `method` so far, when it
    /// reports them.
    fn primary_fetched(&self, method: &str) -> Option<u64> {
        self.primary_fetches
            .as_ref()
            .map(|fetches| fetches.count(method))
    }

    fn degraded(&self) -> MutexGuard<'_, BTreeMap<&'static str, (String, Instant)>> {
        // A panic aborts the process, so poisoning only shows up in tests.
        self.degraded.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record that `method` is served by the scraper because GraphQL failed.
    fn note_fallback(&self, method: &'static str, err: &AirbnbError) {
        log_fallback(method, err);
        let error: String = err
            .to_string()
            .chars()
            .take(DEGRADATION_ERROR_CHARS)
            .collect();
        self.degraded().insert(method, (error, Instant::now()));
    }

    /// Record that GraphQL answered `method` again. `fetched_before` is
    /// [`Self::primary_fetched`] read before the call: an answer that did
    /// not raise it came from the cache and says nothing about the source,
    /// so the fallback stays reported.
    fn note_primary_ok(&self, method: &'static str, fetched_before: Option<u64>) {
        let from_source = match (fetched_before, self.primary_fetched(method)) {
            (Some(before), Some(now)) => now > before,
            _ => true,
        };
        if from_source {
            self.degraded().remove(method);
        }
    }
}

/// Whether a GraphQL failure justifies a second request through the HTML scraper.
///
/// Fall back only when the GraphQL *source* is broken: transport, HTTP status,
/// an unparseable or drifted payload. Never fall back when a second request
/// cannot help or would be impolite: invalid input, a listing that does not
/// exist, a business listing without a host profile (the HTML page is the same
/// document), a 429 from Airbnb, missing data, or a local configuration problem.
pub fn should_fall_back(err: &AirbnbError) -> bool {
    match err {
        AirbnbError::Http { .. }
        | AirbnbError::UpstreamStatus { .. }
        | AirbnbError::Parse { .. }
        | AirbnbError::UpstreamSchema { .. }
        | AirbnbError::Json(_) => true,
        AirbnbError::ListingNotFound { .. }
        | AirbnbError::HostProfileUnavailable { .. }
        | AirbnbError::RateLimited
        | AirbnbError::InvalidParams { .. }
        | AirbnbError::InsufficientData { .. }
        | AirbnbError::AllSourcesFailed { .. }
        | AirbnbError::Config(_)
        | AirbnbError::Io(_)
        | AirbnbError::Yaml(_) => false,
    }
}

fn both_failed(primary: AirbnbError, fallback: AirbnbError) -> AirbnbError {
    AirbnbError::AllSourcesFailed {
        primary: Box::new(primary),
        fallback: Box::new(fallback),
    }
}

fn log_fallback(method: &str, err: &AirbnbError) {
    // escape_debug: error text may contain caller input (log forging).
    warn!(
        error = %err.to_string().escape_debug(),
        method,
        "GraphQL failed, falling back to HTML scraper"
    );
}

/// The number of reviews a page's summary claims when the page carries no
/// review text, or `None` when it has reviews or claims none. Such a page
/// is drift (a missing reviews array, a rotated hash answered by the listing
/// page's rating block), never a successful empty result.
fn claimed_reviews_without_text(page: &ReviewsPage) -> Option<u32> {
    if !page.reviews.is_empty() {
        return None;
    }
    page.summary
        .as_ref()
        .map(|summary| summary.total_reviews)
        .filter(|&total| total > 0)
}

fn html_summary_without_text() -> AirbnbError {
    AirbnbError::UpstreamSchema {
        operation: "reviews page (HTML)".into(),
        detail: "listing page carries the rating summary but no review text".into(),
    }
}

/// Fill the gaps of a GraphQL detail with values from the HTML page.
fn merge_detail(gql: &mut ListingDetail, scraped: ListingDetail) {
    let scraped_price = scraped.known_price();
    if gql.name.is_empty() && !scraped.name.is_empty() {
        gql.name = scraped.name;
    }
    if gql.location.is_empty() && !scraped.location.is_empty() {
        gql.location = scraped.location;
    }
    if gql.description.is_empty() && !scraped.description.is_empty() {
        gql.description = scraped.description;
    }
    if gql.amenities.is_empty() && !scraped.amenities.is_empty() {
        gql.amenities = scraped.amenities;
    }
    if gql.photos.is_empty() && !scraped.photos.is_empty() {
        gql.photos = scraped.photos;
    }
    if gql.house_rules.is_empty() && !scraped.house_rules.is_empty() {
        gql.house_rules = scraped.house_rules;
    }
    if gql.host_name.is_none() {
        gql.host_name = scraped.host_name;
    }
    if gql.known_price().is_none()
        && let Some(price) = scraped_price
    {
        gql.price_per_night = price;
        gql.currency = scraped.currency;
    }
    if gql.rating.is_none() {
        gql.rating = scraped.rating;
    }
    if gql.review_count == 0 {
        gql.review_count = scraped.review_count;
    }
    if gql.host_id.is_none() {
        gql.host_id = scraped.host_id;
    }
}

impl CompositeClient {
    /// An empty first GraphQL reviews page: let the listing page stand in
    /// for it, and never answer with an empty page while either source's
    /// summary says the listing has reviews.
    async fn complete_empty_first_reviews_page(
        &self,
        id: &str,
        gql_page: ReviewsPage,
    ) -> Result<ReviewsPage> {
        let gql_claim = claimed_reviews_without_text(&gql_page);
        match self.scraper.get_reviews(id, None).await {
            Ok(scraped) if !scraped.reviews.is_empty() => Ok(scraped),
            Ok(scraped) => {
                let scraped_claim = claimed_reviews_without_text(&scraped);
                if let Some(claimed) = gql_claim.or(scraped_claim) {
                    let fallback = if scraped_claim.is_some() {
                        html_summary_without_text()
                    } else {
                        AirbnbError::UpstreamSchema {
                            operation: "reviews page (HTML)".into(),
                            detail: "listing page has no review text".into(),
                        }
                    };
                    return Err(both_failed(empty_first_page(claimed), fallback));
                }
                // No source claims any review: a genuinely empty listing.
                if gql_page.summary.is_none() && scraped.summary.is_some() {
                    return Ok(scraped);
                }
                Ok(gql_page)
            }
            Err(e) => {
                if let Some(claimed) = gql_claim {
                    return Err(both_failed(empty_first_page(claimed), e));
                }
                debug!(
                    error = %e.to_string().escape_debug(),
                    "HTML scraper could not stand in for an empty first reviews page"
                );
                Ok(gql_page)
            }
        }
    }
}

fn empty_first_page(claimed: u32) -> AirbnbError {
    AirbnbError::UpstreamSchema {
        operation: "StaysPdpReviewsQuery".into(),
        detail: format!("first page is empty but the summary claims {claimed} reviews"),
    }
}

/// Try GraphQL; on a fallback-worthy error try the scraper, keeping both causes.
macro_rules! with_fallback {
    ($self:expr, $method:ident $(, $arg:expr)*) => {{
        let fetched_before = $self.primary_fetched(stringify!($method));
        match $self.graphql.$method($($arg),*).await {
            Ok(result) => {
                $self.note_primary_ok(stringify!($method), fetched_before);
                Ok(result)
            }
            Err(e) if should_fall_back(&e) => {
                $self.note_fallback(stringify!($method), &e);
                $self
                    .scraper
                    .$method($($arg),*)
                    .await
                    .map_err(|fallback| both_failed(e, fallback))
            }
            Err(e) => Err(e),
        }
    }};
}

#[async_trait]
impl AirbnbClient for CompositeClient {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        with_fallback!(self, search_listings, params)
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        let fetched_before = self.primary_fetched("get_listing_detail");
        match self.graphql.get_listing_detail(id).await {
            Ok(mut gql) => {
                // Only structural gaps justify a second request. The HTML page
                // has no dates either, so it cannot supply a nightly price, and
                // a missing rating or empty house rules are legitimate.
                let structurally_incomplete = gql.name.is_empty()
                    || gql.location.is_empty()
                    || gql.description.is_empty()
                    || gql.amenities.is_empty()
                    || gql.photos.is_empty();
                if structurally_incomplete {
                    match self.scraper.get_listing_detail(id).await {
                        Ok(scraped) => merge_detail(&mut gql, scraped),
                        Err(e) => debug!(
                            error = %e.to_string().escape_debug(),
                            "HTML scraper could not complete the GraphQL detail"
                        ),
                    }
                }
                self.note_primary_ok("get_listing_detail", fetched_before);
                Ok(gql)
            }
            Err(e) if should_fall_back(&e) => {
                self.note_fallback("get_listing_detail", &e);
                self.scraper
                    .get_listing_detail(id)
                    .await
                    .map_err(|fallback| both_failed(e, fallback))
            }
            Err(e) => Err(e),
        }
    }

    /// Only the GraphQL source quotes a stay (the HTML page has no dated
    /// price), so there is nothing to fall back to.
    async fn get_price_quote(&self, request: &QuoteRequest) -> Result<PriceQuote> {
        self.graphql.get_price_quote(request).await
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        let fetched_before = self.primary_fetched("get_reviews");
        match self.graphql.get_reviews(id, cursor).await {
            Ok(gql_page) => {
                // The HTML scraper only sees the reviews embedded in the listing page
                // (the first page). It may stand in for an empty *first* GraphQL page,
                // never for a later page reached through an offset cursor.
                self.note_primary_ok("get_reviews", fetched_before);
                if cursor.is_none() && gql_page.reviews.is_empty() {
                    return self.complete_empty_first_reviews_page(id, gql_page).await;
                }
                Ok(gql_page)
            }
            // A GraphQL cursor means nothing to the scraper: later pages never fall back.
            Err(e) if cursor.is_none() && should_fall_back(&e) => {
                self.note_fallback("get_reviews", &e);
                match self.scraper.get_reviews(id, None).await {
                    // The listing page's rating summary alone is not a reviews page.
                    Ok(scraped) if claimed_reviews_without_text(&scraped).is_some() => {
                        Err(both_failed(e, html_summary_without_text()))
                    }
                    Ok(scraped) => Ok(scraped),
                    Err(fallback) => Err(both_failed(e, fallback)),
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        with_fallback!(self, get_price_calendar, id, months)
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        with_fallback!(self, get_host_profile, listing_id)
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        with_fallback!(self, get_neighborhood_stats, params)
    }

    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate> {
        with_fallback!(self, get_occupancy_estimate, id, months)
    }

    fn clear_cache(&self) {
        self.graphql.clear_cache();
        self.scraper.clear_cache();
    }

    /// The operations the scraper is standing in for. An entry disappears
    /// when GraphQL answers the operation again over the network (a cache
    /// hit does not count), or after 15 minutes.
    fn degradations(&self) -> Vec<Degradation> {
        let mut degraded = self.degraded();
        degraded.retain(|_, (_, at)| at.elapsed() < DEGRADATION_TTL);
        degraded
            .iter()
            .map(|(operation, (error, _))| Degradation {
                operation: (*operation).to_string(),
                error: error.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::review::ReviewsSummary;
    use crate::test_helpers::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_composite(graphql: MockAirbnbClient, scraper: MockAirbnbClient) -> CompositeClient {
        CompositeClient::new(Box::new(graphql), Box::new(scraper))
    }

    fn drifted() -> AirbnbError {
        AirbnbError::UpstreamSchema {
            operation: "StaysPdpSections".into(),
            detail: "HTTP 400 deprecated_web_operation".into(),
        }
    }

    #[tokio::test]
    async fn a_fallback_is_reported_until_graphql_answers_again() {
        let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = Arc::clone(&failing);
        let graphql = MockAirbnbClient::new().with_detail(move |id| {
            if flag.load(Ordering::SeqCst) {
                Err(drifted())
            } else {
                Ok(make_listing_detail(id))
            }
        });
        let composite = make_composite(graphql, MockAirbnbClient::new());
        assert_eq!(composite.degradations(), Vec::<Degradation>::new());

        composite.get_listing_detail("42").await.unwrap();
        let degraded = composite.degradations();
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].operation, "get_listing_detail");
        assert_eq!(degraded[0].label(), "listing details");
        assert!(
            degraded[0].error.contains("deprecated_web_operation"),
            "{degraded:?}"
        );

        failing.store(false, Ordering::SeqCst);
        composite.get_listing_detail("42").await.unwrap();
        assert!(
            composite.degradations().is_empty(),
            "cleared once GraphQL answers"
        );
    }

    #[tokio::test]
    async fn a_cached_answer_does_not_clear_a_reported_fallback() {
        let fetches = Arc::new(FetchCounter::default());
        let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let from_network = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let graphql = MockAirbnbClient::new()
            .with_detail({
                let (failing, from_network, fetches) = (
                    Arc::clone(&failing),
                    Arc::clone(&from_network),
                    Arc::clone(&fetches),
                );
                move |id| {
                    if failing.load(Ordering::SeqCst) {
                        return Err(drifted());
                    }
                    if from_network.load(Ordering::SeqCst) {
                        fetches.record("get_listing_detail");
                    }
                    Ok(make_listing_detail(id))
                }
            })
            .with_calendar({
                let (failing, from_network, fetches) = (
                    Arc::clone(&failing),
                    Arc::clone(&from_network),
                    Arc::clone(&fetches),
                );
                move |id, _| {
                    if failing.load(Ordering::SeqCst) {
                        return Err(drifted());
                    }
                    if from_network.load(Ordering::SeqCst) {
                        fetches.record("get_price_calendar");
                    }
                    Ok(make_price_calendar(id, vec![]))
                }
            });
        let composite = make_composite(graphql, MockAirbnbClient::new())
            .with_primary_fetches(Arc::clone(&fetches));

        // Hand-written path (detail) and macro path (calendar).
        composite.get_listing_detail("42").await.unwrap();
        composite.get_price_calendar("42", 3).await.unwrap();
        assert_eq!(composite.degradations().len(), 2);

        // GraphQL says Ok without fetching: its cache answered.
        failing.store(false, Ordering::SeqCst);
        composite.get_listing_detail("7").await.unwrap();
        composite.get_price_calendar("7", 3).await.unwrap();
        assert_eq!(
            composite.degradations().len(),
            2,
            "a cache hit says nothing about the source"
        );

        // A network answer of one operation clears that operation only.
        from_network.store(true, Ordering::SeqCst);
        composite.get_listing_detail("42").await.unwrap();
        let left = composite.degradations();
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].operation, "get_price_calendar");
        composite.get_price_calendar("42", 3).await.unwrap();
        assert_eq!(composite.degradations(), Vec::<Degradation>::new());
    }

    #[tokio::test]
    async fn clearing_the_cache_reaches_both_sources() {
        let graphql = MockAirbnbClient::new();
        let scraper = MockAirbnbClient::new();
        let (primary, fallback) = (graphql.cache_clears(), scraper.cache_clears());
        make_composite(graphql, scraper).clear_cache();
        assert_eq!(primary.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fallbacks_of_the_macro_methods_are_reported_and_errors_are_capped() {
        let graphql = MockAirbnbClient::new()
            .with_calendar(|_, _| {
                Err(AirbnbError::Parse {
                    reason: "x".repeat(1000),
                })
            })
            .with_search(|_| Err(AirbnbError::RateLimited));
        let composite = make_composite(graphql, MockAirbnbClient::new());
        composite.get_price_calendar("42", 3).await.unwrap();
        // A 429 is returned as-is: no fallback, so nothing to report.
        assert!(
            composite
                .search_listings(&SearchParams::default())
                .await
                .is_err()
        );
        let degraded = composite.degradations();
        assert_eq!(degraded.len(), 1, "{degraded:?}");
        assert_eq!(degraded[0].operation, "get_price_calendar");
        assert_eq!(degraded[0].error.chars().count(), 300);
    }

    #[tokio::test]
    async fn price_quotes_come_from_graphql_only() {
        let request = QuoteRequest {
            listing_id: "42".into(),
            checkin: "2027-06-12".into(),
            checkout: "2027-06-19".into(),
            adults: 2,
        };
        let scraper_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&scraper_calls);
        let scraper = MockAirbnbClient::new().with_quote(move |request| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(make_price_quote(request, Some(1.0)))
        });
        let ok = make_composite(MockAirbnbClient::new(), scraper);
        assert_eq!(
            ok.get_price_quote(&request).await.unwrap().nightly,
            Some(100.0)
        );

        let failing = MockAirbnbClient::new().with_quote(|_| Err(drifted()));
        let broken = make_composite(failing, MockAirbnbClient::new());
        let err = broken.get_price_quote(&request).await.unwrap_err();
        assert!(matches!(err, AirbnbError::UpstreamSchema { .. }), "{err}");
        assert_eq!(
            scraper_calls.load(Ordering::SeqCst),
            0,
            "no HTML fallback for quotes"
        );
        assert_eq!(broken.degradations(), Vec::<Degradation>::new());
    }

    #[tokio::test]
    async fn graphql_success_no_fallback() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "1",
                "GQL Result",
                100.0,
            )]))
        });
        // Scraper returns error — should never be called
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
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
            ..SearchParams::default()
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].name, "GQL Result");
    }

    #[tokio::test]
    async fn graphql_error_falls_back_to_scraper() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "2",
                "Scraper Result",
                200.0,
            )]))
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
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
            ..SearchParams::default()
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].name, "Scraper Result");
    }

    #[tokio::test]
    async fn fallback_search() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "1", "Fallback", 50.0,
            )]))
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
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
            ..SearchParams::default()
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings.len(), 1);
    }

    #[tokio::test]
    async fn fallback_detail() {
        let gql = MockAirbnbClient::new().with_detail(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraped Detail".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Scraped Detail");
    }

    #[tokio::test]
    async fn fallback_reviews() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new()
            .with_reviews(|id, _| Ok(make_reviews_page(id, vec![make_review("Alice", "Great!")])));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Alice");
    }

    #[tokio::test]
    async fn fallback_calendar() {
        let gql = MockAirbnbClient::new().with_calendar(|_, _| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_calendar(|id, _| {
            Ok(make_price_calendar(
                id,
                vec![make_calendar_day("2025-06-01", Some(100.0), true)],
            ))
        });
        let composite = make_composite(gql, scraper);
        let cal = composite.get_price_calendar("42", 3).await.unwrap();
        assert_eq!(cal.days.len(), 1);
    }

    #[tokio::test]
    async fn fallback_host_profile() {
        let gql = MockAirbnbClient::new().with_host_profile(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper =
            MockAirbnbClient::new().with_host_profile(|_| Ok(make_host_profile("Scraped Host")));
        let composite = make_composite(gql, scraper);
        let profile = composite.get_host_profile("42").await.unwrap();
        assert_eq!(profile.name, "Scraped Host");
    }

    #[tokio::test]
    async fn fallback_neighborhood_stats() {
        let gql = MockAirbnbClient::new().with_neighborhood(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_neighborhood(|params| {
            let mut stats = make_neighborhood_stats(&params.location);
            stats.total_listings = 42;
            Ok(stats)
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Berlin".into(),
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
            ..SearchParams::default()
        };
        let stats = composite.get_neighborhood_stats(&params).await.unwrap();
        assert_eq!(stats.total_listings, 42);
    }

    #[tokio::test]
    async fn fallback_occupancy_estimate() {
        let gql = MockAirbnbClient::new().with_occupancy(|_, _| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_occupancy(|id, _| {
            let mut est = make_occupancy_estimate(id);
            est.occupancy_rate = 0.75;
            Ok(est)
        });
        let composite = make_composite(gql, scraper);
        let est = composite.get_occupancy_estimate("42", 3).await.unwrap();
        assert!((est.occupancy_rate - 0.75).abs() < f64::EPSILON);
    }

    // --- Detail smart merge tests ---

    /// Helper: creates a GQL detail with specific empty fields to trigger merge
    fn gql_detail_with_empty_name(id: &str) -> ListingDetail {
        let mut d = make_listing_detail(id);
        d.name = String::new(); // triggers merge condition
        d
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_name() {
        let gql = MockAirbnbClient::new().with_detail(|id| Ok(gql_detail_with_empty_name(id)));
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraped Name".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Scraped Name");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_location() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.location = String::new(); // triggers merge condition
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.location = "Scraped City".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.location, "Scraped City");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_amenities() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.amenities = vec![]; // triggers merge condition
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.amenities = vec!["Pool".into(), "Sauna".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.amenities, vec!["Pool", "Sauna"]);
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_description() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.description = String::new();
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.description = "Scraped description".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.description, "Scraped description");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_photos() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.photos = vec![];
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.photos = vec!["https://example.com/photo.jpg".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.photos, vec!["https://example.com/photo.jpg"]);
    }

    #[tokio::test]
    async fn detail_merge_fills_missing_host_name() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.host_name = None;
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.host_name = Some("Scraped Host".into());
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.host_name, Some("Scraped Host".into()));
    }

    #[tokio::test]
    async fn detail_merge_preserves_nonempty_fields() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "GQL Name".into();
            d.location = "GQL City".into();
            d.description = "GQL desc".into();
            d.amenities = vec!["WiFi".into()];
            d.photos = vec!["gql_photo.jpg".into()];
            d.host_name = Some("GQL Host".into());
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraper Name".into();
            d.location = "Scraper City".into();
            d.description = "Scraper desc".into();
            d.amenities = vec!["Pool".into()];
            d.photos = vec!["scraper_photo.jpg".into()];
            d.host_name = Some("Scraper Host".into());
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        // All GQL values preserved — merge condition not triggered
        assert_eq!(detail.name, "GQL Name");
        assert_eq!(detail.location, "GQL City");
        assert_eq!(detail.description, "GQL desc");
        assert_eq!(detail.amenities, vec!["WiFi"]);
        assert_eq!(detail.photos, vec!["gql_photo.jpg"]);
        assert_eq!(detail.host_name, Some("GQL Host".into()));
    }

    #[tokio::test]
    async fn detail_merge_skips_when_all_critical_present() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Present".into();
            d.location = "Present".into();
            d.description = "Present desc".into();
            d.amenities = vec!["Present".into()];
            d.photos = vec!["photo.jpg".into()];
            d.house_rules = vec!["No parties".into()];
            d.price_per_night = 100.0;
            d.rating = Some(4.5);
            Ok(d)
        });
        // Scraper returns error — should never be called since merge condition not met
        let scraper = MockAirbnbClient::new().with_detail(|_| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Present");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_house_rules() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.house_rules = vec![];
            d.description = String::new(); // house rules alone no longer trigger a scraper request
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.house_rules = vec!["No smoking".into(), "No pets".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.house_rules, vec!["No smoking", "No pets"]);
    }

    #[tokio::test]
    async fn detail_merge_scraper_error_returns_gql_only() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge attempt
            d.location = "GQL City".into();
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        // GQL result returned as-is since scraper failed
        assert_eq!(detail.name, "");
        assert_eq!(detail.location, "GQL City");
    }

    // --- Reviews smart merge tests ---

    #[tokio::test]
    async fn reviews_merge_gql_empty_uses_scraper_reviews() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(id, vec![])) // empty reviews
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("Alice", "Great place!")],
            ))
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Alice");
    }

    /// A page with no review text whose summary claims `total` reviews, like
    /// the 2026-09 listing page (`overallCount` 490, no `reviewsData`).
    fn summary_only_page(id: &str, total: u32) -> ReviewsPage {
        let mut page = make_reviews_page(id, vec![]);
        page.summary = Some(ReviewsSummary {
            total_reviews: total,
            ..make_reviews_summary()
        });
        page
    }

    #[tokio::test]
    async fn reviews_merge_gql_empty_no_summary_scraper_summary_only_is_an_error() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 490)));
        let composite = make_composite(gql, scraper);
        let err = composite.get_reviews("42", None).await.unwrap_err();
        assert!(
            matches!(err, AirbnbError::AllSourcesFailed { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn reviews_merge_gql_empty_no_summary_uses_scraper_zero_review_summary() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 0)));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        assert_eq!(page.summary.map(|s| s.total_reviews), Some(0));
    }

    #[tokio::test]
    async fn reviews_merge_gql_has_reviews_ignores_scraper() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("GQL Author", "GQL review")],
            ))
        });
        // Scraper should not be used — error proves it's never called
        let scraper = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "GQL Author");
    }

    #[tokio::test]
    async fn reviews_merge_both_empty_with_a_gql_summary_claiming_reviews_is_an_error() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 50)));
        let scraper =
            MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let composite = make_composite(gql, scraper);
        let err = composite.get_reviews("42", None).await.unwrap_err();
        let AirbnbError::AllSourcesFailed { primary, .. } = err else {
            panic!("expected AllSourcesFailed, got {err:?}");
        };
        assert!(
            matches!(*primary, AirbnbError::UpstreamSchema { .. }),
            "got {primary:?}"
        );
    }

    #[tokio::test]
    async fn reviews_merge_both_genuinely_empty_returns_gql() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 0)));
        let scraper =
            MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        assert_eq!(page.summary.map(|s| s.total_reviews), Some(0));
    }

    #[tokio::test]
    async fn reviews_merge_both_empty_without_summaries_is_ok_empty() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let scraper =
            MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.summary.is_none());
    }

    #[tokio::test]
    async fn reviews_gql_schema_error_with_a_summary_only_scraper_page_is_all_sources_failed() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::UpstreamSchema {
                operation: "StaysPdpReviewsQuery".into(),
                detail: "HTTP 422: stale persisted query".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 490)));
        let composite = make_composite(gql, scraper);
        let err = composite.get_reviews("42", None).await.unwrap_err();
        let AirbnbError::AllSourcesFailed { primary, fallback } = err else {
            panic!("expected AllSourcesFailed, got {err:?}");
        };
        assert!(
            matches!(*primary, AirbnbError::UpstreamSchema { .. }),
            "got {primary:?}"
        );
        assert!(
            matches!(*fallback, AirbnbError::UpstreamSchema { ref operation, .. } if operation == "reviews page (HTML)"),
            "got {fallback:?}"
        );
    }

    #[tokio::test]
    async fn reviews_gql_error_with_a_zero_review_scraper_page_is_ok_empty() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::UpstreamSchema {
                operation: "StaysPdpReviewsQuery".into(),
                detail: "HTTP 422".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 0)));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
    }

    #[tokio::test]
    async fn reviews_gql_error_with_a_summaryless_empty_scraper_page_is_ok_empty() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::UpstreamSchema {
                operation: "StaysPdpReviewsQuery".into(),
                detail: "HTTP 422".into(),
            })
        });
        let scraper =
            MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.summary.is_none());
    }

    #[tokio::test]
    async fn reviews_empty_first_page_scraper_error_returns_gql_page() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let scraper = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "listing page unreachable".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.summary.is_none());
    }

    #[tokio::test]
    async fn reviews_empty_first_page_claiming_reviews_with_a_scraper_error_is_an_error() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(summary_only_page(id, 490)));
        let scraper = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "listing page unreachable".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let err = composite.get_reviews("42", None).await.unwrap_err();
        assert!(
            matches!(err, AirbnbError::AllSourcesFailed { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn upstream_schema_error_falls_back_to_scraper() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::UpstreamSchema {
                operation: "StaysSearch".into(),
                detail: "stale persisted query".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "7",
                "From HTML",
                90.0,
            )]))
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Paris".into(),
            ..SearchParams::default()
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].name, "From HTML");
    }

    #[tokio::test]
    async fn reviews_later_empty_page_is_not_replaced_by_the_scraper_first_page() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| Ok(make_reviews_page(id, vec![])));
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("First", "first-page review")],
            ))
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", Some("24")).await.unwrap();
        assert!(
            page.reviews.is_empty(),
            "a later GraphQL page must not be replaced by the scraper's first page"
        );
    }

    #[tokio::test]
    async fn reviews_later_page_error_is_returned_not_replaced() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "page 3 failed".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("First", "first-page review")],
            ))
        });
        let composite = make_composite(gql, scraper);
        let err = composite.get_reviews("42", Some("48")).await.unwrap_err();
        assert!(err.to_string().contains("page 3 failed"), "{err}");
    }

    #[tokio::test]
    async fn reviews_first_page_fallback_calls_the_scraper_without_cursor() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "gql down".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, cursor| {
            if cursor.is_some() {
                return Err(AirbnbError::Parse {
                    reason: "scraper received a GraphQL cursor".into(),
                });
            }
            Ok(make_reviews_page(id, vec![make_review("Alice", "Great!")]))
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
    }

    #[tokio::test]
    async fn detail_merge_replaces_an_unknown_nan_price_with_the_scraped_one() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.price_per_night = f64::NAN;
            d.description = String::new(); // a missing price alone no longer triggers a scraper request (DECISION P3-8)
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.price_per_night = 95.0;
            d.currency = "\u{20ac}".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.known_price(), Some(95.0));
        assert_eq!(detail.currency, "\u{20ac}");
    }

    #[tokio::test]
    async fn host_profile_unavailable_is_not_retried_on_the_scraper() {
        let gql = MockAirbnbClient::new().with_host_profile(|id| {
            Err(AirbnbError::HostProfileUnavailable {
                listing_id: id.to_string(),
                reason: "offered by a business (pdpType HOTEL)".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_host_profile(|_| {
            Err(AirbnbError::Parse {
                reason: "scraper must not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let err = composite.get_host_profile("42").await.unwrap_err();
        assert!(
            matches!(err, AirbnbError::HostProfileUnavailable { .. }),
            "{err}"
        );
    }

    fn paris() -> SearchParams {
        SearchParams {
            location: "Paris".into(),
            ..SearchParams::default()
        }
    }

    fn counting_scraper_search(calls: &Arc<AtomicUsize>) -> MockAirbnbClient {
        let seen = Arc::clone(calls);
        MockAirbnbClient::new().with_search(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(make_search_result(vec![make_listing(
                "2",
                "Scraper Result",
                200.0,
            )]))
        })
    }

    fn counting_scraper_detail(calls: &Arc<AtomicUsize>) -> MockAirbnbClient {
        let seen = Arc::clone(calls);
        MockAirbnbClient::new().with_detail(move |id| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(make_listing_detail(id))
        })
    }

    #[test]
    fn fallback_policy_table() {
        let fall_back = [
            AirbnbError::Http {
                reason: "timeout".into(),
            },
            AirbnbError::UpstreamStatus {
                status: 503,
                context: "GraphQL StaysSearch".into(),
            },
            AirbnbError::Parse {
                reason: "unexpected shape".into(),
            },
            AirbnbError::UpstreamSchema {
                operation: "StaysSearch".into(),
                detail: "ValidationError".into(),
            },
        ];
        for err in &fall_back {
            assert!(should_fall_back(err), "{err} should fall back");
        }
        let stop = [
            AirbnbError::RateLimited,
            AirbnbError::InvalidParams {
                reason: "bad id".into(),
            },
            AirbnbError::ListingNotFound { id: "1".into() },
            AirbnbError::HostProfileUnavailable {
                listing_id: "1".into(),
                reason: "offered by a business".into(),
            },
            AirbnbError::InsufficientData {
                reason: "no comparable listing".into(),
            },
            AirbnbError::Config("bad base_url".into()),
        ];
        for err in &stop {
            assert!(!should_fall_back(err), "{err} must not fall back");
        }
    }

    #[tokio::test]
    async fn rate_limited_is_returned_without_touching_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let composite = make_composite(gql, counting_scraper_search(&calls));
        let err = composite.search_listings(&paris()).await.unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a 429 must not trigger a second request"
        );
    }

    #[tokio::test]
    async fn invalid_params_is_returned_without_touching_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::InvalidParams {
                reason: "location is required".into(),
            })
        });
        let composite = make_composite(gql, counting_scraper_search(&calls));
        let err = composite.search_listings(&paris()).await.unwrap_err();
        assert!(matches!(err, AirbnbError::InvalidParams { .. }), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn listing_not_found_is_returned_without_touching_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new()
            .with_detail(|id| Err(AirbnbError::ListingNotFound { id: id.to_string() }));
        let composite = make_composite(gql, counting_scraper_detail(&calls));
        let err = composite.get_listing_detail("42").await.unwrap_err();
        assert!(matches!(err, AirbnbError::ListingNotFound { .. }), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn upstream_schema_drift_falls_back_to_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::UpstreamSchema {
                operation: "StaysSearch".into(),
                detail: "ValidationError".into(),
            })
        });
        let composite = make_composite(gql, counting_scraper_search(&calls));
        let result = composite.search_listings(&paris()).await.unwrap();
        assert_eq!(result.listings[0].name, "Scraper Result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn both_fail_reports_both_causes() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "stale persisted-query hash".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "layout changed".into(),
            })
        });
        let err = make_composite(gql, scraper)
            .search_listings(&paris())
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::AllSourcesFailed { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("stale persisted-query hash"), "{msg}");
        assert!(msg.contains("layout changed"), "{msg}");
    }

    #[tokio::test]
    async fn detail_with_unknown_price_and_no_rating_does_not_hit_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.price_per_night = 0.0; // undated GraphQL detail: price unknown (I1)
            d.rating = None;
            d.house_rules = vec![];
            Ok(d)
        });
        let composite = make_composite(gql, counting_scraper_detail(&calls));
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no second request for price, rating or house rules"
        );
        assert!(detail.known_price().is_none());
    }

    #[tokio::test]
    async fn detail_merge_takes_only_a_known_scraper_price() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.description = String::new(); // structural gap: triggers the merge
            d.price_per_night = 0.0;
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.price_per_night = 80.0;
            d.currency = "EUR".into();
            Ok(d)
        });
        let detail = make_composite(gql, scraper)
            .get_listing_detail("42")
            .await
            .unwrap();
        assert_eq!(detail.known_price(), Some(80.0));
        assert_eq!(detail.currency, "EUR");
    }

    #[tokio::test]
    async fn detail_merge_never_copies_an_unknown_scraper_price() {
        let gql_currency = make_listing_detail("42").currency;
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.description = String::new();
            d.price_per_night = 0.0;
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.price_per_night = 0.0;
            d.currency = "EUR".into();
            Ok(d)
        });
        let detail = make_composite(gql, scraper)
            .get_listing_detail("42")
            .await
            .unwrap();
        assert!(detail.known_price().is_none());
        assert_eq!(detail.currency, gql_currency);
    }

    #[tokio::test]
    async fn reviews_later_page_error_is_not_retried_on_the_scraper() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gql = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let seen = Arc::clone(&calls);
        let scraper = MockAirbnbClient::new().with_reviews(move |id, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(make_reviews_page(id, vec![make_review("Guest A", "fine")]))
        });
        let err = make_composite(gql, scraper)
            .get_reviews("42", Some("24"))
            .await
            .unwrap_err();
        assert!(matches!(err, AirbnbError::Parse { .. }), "{err}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a GraphQL offset cursor must not be sent to the scraper"
        );
    }
}
