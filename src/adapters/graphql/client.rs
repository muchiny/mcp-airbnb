use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Datelike;
use reqwest::{Client, StatusCode};
use tracing::{debug, trace, warn};
use url::Url;

use crate::adapters::http::{
    MAX_BODY_BYTES, RetryPolicy, http_client_builder, read_body_capped, send_with_retries,
};
use crate::adapters::price;
use crate::adapters::rate_limiter::RateLimiter;
use crate::adapters::request_locale;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::{CacheConfig, GraphQLHashes, ScraperConfig};
use crate::domain::analytics::{self, HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::listing_id::validate_listing_id;
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

use super::parsers;

/// `X-Airbnb-GraphQL-Platform` value sent by the airbnb.com web client (captured 2026-09).
const GRAPHQL_PLATFORM: &str = "web";
/// `X-Airbnb-GraphQL-Platform-Client` value sent by the airbnb.com web client (captured 2026-09).
const GRAPHQL_PLATFORM_CLIENT: &str = "minimalist-niobe";
/// JSON pointer to the section list of a `StaysPdpSections` response.
const PDP_SECTIONS_ROOT: &str = "/data/presentation/stayProductDetailPage/sections/sections";
/// JSON pointer to the month list of a `PdpAvailabilityCalendar` response.
const CALENDAR_ROOT: &str = "/data/merlin/pdpAvailabilityCalendar/calendarMonths";
/// Longest upstream error message copied into an `UpstreamSchema` detail.
const MAX_UPSTREAM_MESSAGE_CHARS: usize = 200;

pub struct AirbnbGraphQLClient {
    http: Client,
    rate_limiter: Arc<RateLimiter>,
    retry: RetryPolicy,
    cache: Arc<dyn ListingCache>,
    base_url: String,
    hashes: GraphQLHashes,
    cache_config: CacheConfig,
    api_key_manager: Arc<ApiKeyManager>,
    /// ISO 4217 currency pinned on every request.
    currency: String,
    /// Locale pinned on every request.
    locale: String,
}

impl AirbnbGraphQLClient {
    /// `rate_limiter` is the process-wide limiter shared with the scraper and
    /// the API-key manager (I3).
    pub fn new(
        config: &ScraperConfig,
        cache_config: CacheConfig,
        cache: Arc<dyn ListingCache>,
        api_key_manager: Arc<ApiKeyManager>,
        rate_limiter: Arc<RateLimiter>,
    ) -> std::result::Result<Self, reqwest::Error> {
        let http = http_client_builder(config).cookie_store(true).build()?;

        Ok(Self {
            http,
            rate_limiter,
            retry: RetryPolicy::from_config(config),
            cache,
            base_url: config.base_url.clone(),
            hashes: config.graphql_hashes.clone(),
            cache_config,
            api_key_manager,
            currency: config.currency.clone(),
            locale: config.locale.clone(),
        })
    }

    /// Execute a GraphQL GET request with persisted query hash.
    async fn graphql_get(
        &self,
        operation_name: &str,
        hash: &str,
        variables: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let extensions = serde_json::json!({
            "persistedQuery": {
                "version": 1,
                "sha256Hash": hash,
            }
        });

        let endpoint = format!("{}/api/v3/{operation_name}/{hash}/", self.base_url);
        let mut url = Url::parse(&endpoint)?;
        url.query_pairs_mut()
            .append_pair("operationName", operation_name)
            .append_pair("locale", &self.locale)
            .append_pair("currency", &self.currency)
            .append_pair("variables", &variables.to_string())
            .append_pair("extensions", &extensions.to_string());

        debug!(url = %url, "GraphQL GET request");

        let request = self.with_web_client_headers(self.http.get(url.as_str()));
        let response = self.execute(operation_name, &request).await?;
        read_graphql_response(operation_name, response).await
    }

    /// Execute a GraphQL POST request (used for search which requires a body).
    ///
    /// Mirrors the airbnb.com web client (2026-09):
    /// `POST /api/v3/{operation}/{hash}?operationName={operation}`.
    async fn graphql_post(
        &self,
        operation_name: &str,
        hash: &str,
        variables: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "operationName": operation_name,
            "variables": variables,
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": hash,
                }
            }
        });

        let mut url = Url::parse(&format!("{}/api/v3/{operation_name}/{hash}", self.base_url))?;
        url.query_pairs_mut()
            .append_pair("operationName", operation_name)
            .append_pair("locale", &self.locale)
            .append_pair("currency", &self.currency);

        debug!(url = %url, "GraphQL POST request");

        let request = self
            .with_web_client_headers(self.http.post(url.as_str()))
            .json(&body);
        let response = self.execute(operation_name, &request).await?;
        read_graphql_response(operation_name, response).await
    }

    /// Add the headers the airbnb.com web client sends on every GraphQL call
    /// (captured 2026-09), with the pinned `Accept-Language` (I4). The API key
    /// is added by [`Self::execute`].
    fn with_web_client_headers(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("X-Airbnb-GraphQL-Platform", GRAPHQL_PLATFORM)
            .header("X-Airbnb-GraphQL-Platform-Client", GRAPHQL_PLATFORM_CLIENT)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header(
                "Accept-Language",
                request_locale::accept_language(&self.locale),
            )
    }

    /// Send one GraphQL request built by the caller (without the API-key
    /// header) through the shared limiter and the retry policy, and return the
    /// final response for `read_graphql_response` to classify (P1a, I2: GraphQL
    /// `errors` in any status and HTTP 422 become `UpstreamSchema`).
    ///
    /// A 401/403 means Airbnb rejected the key: it is invalidated and the
    /// request is sent once more with a fresh key. A second 401/403 is handed
    /// back like any other final status.
    async fn execute(
        &self,
        operation_name: &str,
        request: &reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let context = format!("GraphQL {operation_name}");
        let mut key_refreshed = false;
        loop {
            let api_key = self.api_key_manager.get_api_key().await?;
            let keyed = request
                .try_clone()
                .ok_or_else(|| AirbnbError::Http {
                    reason: format!("{context}: request could not be built or cannot be replayed"),
                })?
                .header("X-Airbnb-Api-Key", &api_key);
            let response =
                send_with_retries(&keyed, &self.rate_limiter, &self.retry, &context).await?;
            let key_rejected = matches!(
                response.status(),
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            );
            if key_rejected && !key_refreshed {
                warn!(
                    operation = operation_name,
                    "Airbnb rejected the API key; fetching a fresh one and retrying once"
                );
                self.api_key_manager.invalidate(&api_key).await;
                key_refreshed = true;
                continue;
            }
            return Ok(response);
        }
    }

    /// `StaysPdpSections` variables. The detail and the host profile read the
    /// same document, so both send the detail request.
    fn pdp_sections_variables(id: &str) -> serde_json::Value {
        let b64 = base64::engine::general_purpose::STANDARD;
        serde_json::json!({
            "id": b64.encode(format!("StayListing:{id}")),
            "demandStayListingId": b64.encode(format!("DemandStayListing:{id}")),
            "pdpSectionsRequest": {
                "adults": "1",
                "bypassTargetings": false,
                "categoryTag": null,
                "children": null,
                "infants": null,
                "layouts": ["SIDEBAR", "SINGLE_COLUMN"],
                "pets": 0,
                "preview": false,
                "previousStateCheckIn": null,
                "previousStateCheckOut": null,
                "privateBooking": false,
                "staysBookingMigrationEnabled": false,
                "useNewSectionWrapperApi": false,
            }
        })
    }

    /// Fetch the `StaysPdpSections` document of a listing. Keeps P1a's root
    /// check, so a document without sections is `UpstreamSchema` before any
    /// parser runs (and `PDP_SECTIONS_ROOT` stays in use).
    async fn fetch_pdp_sections(&self, id: &str) -> Result<serde_json::Value> {
        let json = self
            .graphql_get(
                "StaysPdpSections",
                &self.hashes.stays_pdp_sections,
                &Self::pdp_sections_variables(id),
            )
            .await?;
        require_root(&json, "StaysPdpSections", PDP_SECTIONS_ROOT)?;
        Ok(json)
    }

    fn cache_json<T: serde::Serialize>(&self, key: &str, value: &T, ttl_secs: u64) {
        if let Ok(serialized) = serde_json::to_string(value) {
            self.cache
                .set(key, &serialized, Duration::from_secs(ttl_secs));
        }
    }

    /// Parse the detail out of a `StaysPdpSections` document and cache it.
    fn cache_detail_from(&self, json: &serde_json::Value, id: &str) -> Result<ListingDetail> {
        let mut detail = parsers::detail::parse_detail_response(json, id, &self.base_url)?;
        price::fill_currency(
            &mut detail.currency,
            &price::currency_symbol(&self.currency),
        );
        self.cache_json(
            &format!("gql:detail:{id}"),
            &detail,
            self.cache_config.detail_ttl_secs,
        );
        Ok(detail)
    }

    /// Parse the host profile out of a `StaysPdpSections` document and cache it.
    fn cache_host_from(&self, json: &serde_json::Value, id: &str) -> Result<HostProfile> {
        let profile = parsers::host::parse_host_response(json)?;
        self.cache_json(
            &format!("gql:host:{id}"),
            &profile,
            self.cache_config.host_profile_ttl_secs,
        );
        Ok(profile)
    }
}

/// Read a `/api/v3/{operation}` response and classify failures:
/// HTTP 429 becomes `RateLimited`; a non-empty top-level GraphQL `errors`
/// array (whatever the status) or HTTP 422 becomes `UpstreamSchema` (stale
/// persisted query or variables); any other non-2xx status becomes `Parse`.
async fn read_graphql_response(
    operation_name: &str,
    response: reqwest::Response,
) -> Result<serde_json::Value> {
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(AirbnbError::RateLimited);
    }

    let body = read_body_capped(response, MAX_BODY_BYTES).await?;
    debug!(
        operation = operation_name,
        status = %status,
        body_len = body.len(),
        "GraphQL response received"
    );
    trace!(
        operation = operation_name,
        body = %body,
        "GraphQL raw response"
    );

    let parsed = serde_json::from_str::<serde_json::Value>(&body);
    if let Ok(json) = &parsed
        && let Some(errors) = graphql_errors_detail(json)
    {
        return Err(upstream_schema(
            operation_name,
            &format!("HTTP {status}, GraphQL errors: {errors}"),
        ));
    }
    if status == StatusCode::UNPROCESSABLE_ENTITY {
        return Err(upstream_schema(
            operation_name,
            "HTTP 422 Unprocessable Entity: persisted query rejected",
        ));
    }
    if !status.is_success() {
        return Err(AirbnbError::UpstreamStatus {
            status: status.as_u16(),
            context: format!("GraphQL {operation_name}"),
        });
    }
    parsed.map_err(|e| AirbnbError::Parse {
        reason: format!("GraphQL {operation_name} JSON parse error: {e}"),
    })
}

/// Summarise a non-empty top-level GraphQL `errors` array as
/// `"message [classification]; …"` (at most three entries), or `None`.
fn graphql_errors_detail(json: &serde_json::Value) -> Option<String> {
    let errors = json.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }
    let parts: Vec<String> = errors
        .iter()
        .take(3)
        .map(|error| {
            let message: String = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("no message")
                .chars()
                .take(MAX_UPSTREAM_MESSAGE_CHARS)
                .collect();
            let kind = error
                .pointer("/extensions/classification")
                .or_else(|| error.pointer("/extensions/code"))
                .and_then(serde_json::Value::as_str);
            if let Some(kind) = kind {
                format!("{message} [{kind}]")
            } else {
                message
            }
        })
        .collect();
    Some(parts.join("; "))
}

/// Build the `UpstreamSchema` error for `operation_name`, naming the config key to refresh.
fn upstream_schema(operation_name: &str, detail: &str) -> AirbnbError {
    AirbnbError::UpstreamSchema {
        operation: operation_name.to_string(),
        detail: format!(
            "{detail} (if Airbnb rotated this persisted query, update scraper.graphql_hashes.{} in config.yaml)",
            hash_config_key(operation_name)
        ),
    }
}

/// `graphql_hashes` config key holding the persisted-query hash of `operation_name`.
fn hash_config_key(operation_name: &str) -> &'static str {
    match operation_name {
        "StaysSearch" => "stays_search",
        "StaysPdpSections" => "stays_pdp_sections",
        "StaysPdpReviewsQuery" => "stays_pdp_reviews",
        "PdpAvailabilityCalendar" => "pdp_availability_calendar",
        _ => "<operation>",
    }
}

/// Fail with `UpstreamSchema` when the response lacks the node the parser starts from.
fn require_root(json: &serde_json::Value, operation_name: &str, pointer: &str) -> Result<()> {
    if json.pointer(pointer).is_some_and(|node| !node.is_null()) {
        Ok(())
    } else {
        Err(upstream_schema(
            operation_name,
            &format!("response has no `{pointer}`"),
        ))
    }
}

/// `PdpAvailabilityCalendar` variables as sent by the airbnb.com web client (captured 2026-09).
fn build_calendar_variables(
    listing_id: &str,
    months: u32,
    anchor: chrono::NaiveDate,
) -> serde_json::Value {
    serde_json::json!({
        "request": {
            "count": months,
            "listingId": listing_id,
            "month": anchor.month(),
            "year": anchor.year(),
            "returnPropertyLevelCalendarIfApplicable": false,
        }
    })
}

/// GraphQL review cursors are the numeric offset handed out with the previous page.
fn parse_review_cursor(cursor: Option<&str>) -> Result<u64> {
    let Some(raw) = cursor else {
        return Ok(0);
    };
    raw.trim()
        .parse::<u64>()
        .map_err(|_| AirbnbError::InvalidParams {
            reason: "invalid reviews cursor: expected the numeric offset returned with the previous page"
                .into(),
        })
}

#[async_trait]
impl AirbnbClient for AirbnbGraphQLClient {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        params.validate()?;

        let cache_key = format!("gql:search:{}", params.cache_key());
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(result) = serde_json::from_str::<SearchResult>(&cached)
        {
            debug!("Cache hit for GraphQL search");
            return Ok(result);
        }

        let variables = parsers::search::build_search_variables(params)?;
        let json = self
            .graphql_post("StaysSearch", &self.hashes.stays_search, &variables)
            .await?;
        let mut result =
            parsers::search::parse_search_page(&json, &self.base_url, params.cursor.as_deref())?;
        price::fill_search_currency(&mut result, &price::currency_symbol(&self.currency));

        if let Ok(serialized) = serde_json::to_string(&result) {
            self.cache.set(
                &cache_key,
                &serialized,
                Duration::from_secs(self.cache_config.search_ttl_secs),
            );
        }

        Ok(result)
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        validate_listing_id(id)?;
        if let Some(cached) = self.cache.get(&format!("gql:detail:{id}"))
            && let Ok(detail) = serde_json::from_str::<ListingDetail>(&cached)
        {
            debug!(id, "Cache hit for GraphQL listing detail");
            return Ok(detail);
        }

        let json = self.fetch_pdp_sections(id).await?;
        let detail = self.cache_detail_from(&json, id)?;
        // Same document: cache the host profile too, so a host-profile call
        // for this listing does not fetch the page again.
        if let Err(e) = self.cache_host_from(&json, id) {
            debug!(id, error = %e, "PDP document carries no host profile");
        }
        Ok(detail)
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        validate_listing_id(id)?;
        let offset = parse_review_cursor(cursor)?;
        let cache_key = format!("gql:reviews:{id}:o={offset}");
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(page) = serde_json::from_str::<ReviewsPage>(&cached)
        {
            debug!(id, "Cache hit for GraphQL reviews");
            return Ok(page);
        }

        let variables = parsers::review::build_reviews_variables(id, offset);
        let json = self
            .graphql_get(
                "StaysPdpReviewsQuery",
                &self.hashes.stays_pdp_reviews,
                &variables,
            )
            .await?;
        let page = parsers::review::parse_reviews_page(&json, id, offset)?;

        if let Ok(serialized) = serde_json::to_string(&page) {
            self.cache.set(
                &cache_key,
                &serialized,
                Duration::from_secs(self.cache_config.reviews_ttl_secs),
            );
        }

        Ok(page)
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        validate_listing_id(id)?;
        // One reference date for the request window, the cache key and the
        // past-day labels. Airbnb's calendar carries no time zone, so the
        // operator's local date is the best available anchor.
        let today = chrono::Local::now().date_naive();
        let cache_key = format!("gql:calendar:{id}:{}:m={months}", today.format("%Y-%m"));
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(mut calendar) = serde_json::from_str::<PriceCalendar>(&cached)
        {
            debug!(id, "Cache hit for GraphQL calendar");
            calendar.classify_past_days(today);
            return Ok(calendar);
        }

        // The request window starts at the same local `today`.
        let variables = build_calendar_variables(id, months, today);

        let json = self
            .graphql_get(
                "PdpAvailabilityCalendar",
                &self.hashes.pdp_availability_calendar,
                &variables,
            )
            .await?;
        require_root(&json, "PdpAvailabilityCalendar", CALENDAR_ROOT)?;

        // Parse the decoded JSON directly: no re-serialization, no HTML parse.
        let mut calendar =
            crate::adapters::scraper::calendar_parser::parse_calendar_json(&json, id)?;
        price::fill_currency(
            &mut calendar.currency,
            &price::currency_symbol(&self.currency),
        );
        // Label past days before the calendar is cached or returned.
        calendar.classify_past_days(today);

        if let Ok(serialized) = serde_json::to_string(&calendar) {
            self.cache.set(
                &cache_key,
                &serialized,
                Duration::from_secs(self.cache_config.calendar_ttl_secs),
            );
        }

        Ok(calendar)
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        validate_listing_id(listing_id)?;
        if let Some(cached) = self.cache.get(&format!("gql:host:{listing_id}"))
            && let Ok(profile) = serde_json::from_str::<HostProfile>(&cached)
        {
            debug!(listing_id, "Cache hit for GraphQL host profile");
            return Ok(profile);
        }

        let json = self.fetch_pdp_sections(listing_id).await?;
        if let Err(e) = self.cache_detail_from(&json, listing_id) {
            debug!(listing_id, error = %e, "PDP document carries no parsable detail");
        }
        self.cache_host_from(&json, listing_id)
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        let result = self.search_listings(params).await?;
        Ok(analytics::compute_neighborhood_stats(
            &params.location,
            &result.listings,
        ))
    }

    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate> {
        let calendar = self.get_price_calendar(id, months).await?;
        Ok(analytics::compute_occupancy_estimate(id, &calendar))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::cache::memory_cache::MemoryCache;
    use crate::config::types::CacheConfig;
    use crate::domain::calendar::UnavailabilityReason;
    use crate::test_helpers::{
        make_calendar_day, make_host_profile, make_listing_detail, make_price_calendar,
        make_reviews_page, make_search_result,
    };

    /// Builds a fresh `AirbnbGraphQLClient` backed by a dedicated `MemoryCache`.
    /// Tests populate the cache with the exact key each method uses before
    /// calling it — a cache hit short-circuits the HTTP path entirely, so no
    /// network I/O is attempted in the unit-test environment.
    fn make_client() -> (AirbnbGraphQLClient, Arc<MemoryCache>) {
        let cache = Arc::new(MemoryCache::new(64));
        let scraper = ScraperConfig::default();
        let api_key_manager = Arc::new(ApiKeyManager::new(
            Client::new(),
            scraper.base_url.clone(),
            scraper.api_key_cache_secs,
            Arc::new(RateLimiter::new(100.0)),
        ));
        let client = AirbnbGraphQLClient::new(
            &scraper,
            CacheConfig::default(),
            cache.clone() as Arc<dyn ListingCache>,
            api_key_manager,
            Arc::new(RateLimiter::new(100.0)),
        )
        .expect("graphql client constructs");
        (client, cache)
    }

    #[tokio::test]
    async fn search_listings_returns_cached_result() {
        let (client, cache) = make_client();
        let params = SearchParams {
            location: "Paris".into(),
            ..Default::default()
        };
        let expected = make_search_result(vec![]);
        cache.set(
            &format!("gql:search:{}", params.cache_key()),
            &serde_json::to_string(&expected).unwrap(),
            Duration::from_secs(60),
        );
        let result = client.search_listings(&params).await.unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&expected).unwrap(),
        );
    }

    #[tokio::test]
    async fn get_listing_detail_returns_cached_detail() {
        let (client, cache) = make_client();
        let expected = make_listing_detail("12345");
        cache.set(
            "gql:detail:12345",
            &serde_json::to_string(&expected).unwrap(),
            Duration::from_secs(60),
        );
        let result = client.get_listing_detail("12345").await.unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&expected).unwrap(),
        );
    }

    #[tokio::test]
    async fn get_reviews_returns_cached_page() {
        let (client, cache) = make_client();
        let expected = make_reviews_page("12345", vec![]);
        // cursor None means offset 0: key suffix "o=0"
        cache.set(
            "gql:reviews:12345:o=0",
            &serde_json::to_string(&expected).unwrap(),
            Duration::from_secs(60),
        );
        let result = client.get_reviews("12345", None).await.unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&expected).unwrap(),
        );
    }

    #[tokio::test]
    async fn get_price_calendar_returns_cached_calendar() {
        let (client, cache) = make_client();
        let expected = make_price_calendar("12345", vec![]);
        let month = chrono::Local::now().date_naive().format("%Y-%m");
        cache.set(
            &format!("gql:calendar:12345:{month}:m=3"),
            &serde_json::to_string(&expected).unwrap(),
            Duration::from_secs(60),
        );
        let result = client.get_price_calendar("12345", 3).await.unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&expected).unwrap(),
        );
    }

    #[tokio::test]
    async fn get_price_calendar_relabels_past_days_on_cache_hit() {
        let (client, cache) = make_client();
        let today = chrono::Local::now().date_naive();
        let yesterday = today.pred_opt().unwrap();
        let tomorrow = today.succ_opt().unwrap();
        let mut stale = make_calendar_day(&tomorrow.format("%Y-%m-%d").to_string(), None, false);
        stale.unavailability_reason = Some(UnavailabilityReason::PastDate);
        let mut yesterday_day =
            make_calendar_day(&yesterday.format("%Y-%m-%d").to_string(), None, false);
        yesterday_day.unavailability_reason = Some(UnavailabilityReason::Unknown);
        let cached = make_price_calendar("12345", vec![yesterday_day, stale]);
        cache.set(
            &format!("gql:calendar:12345:{}:m=3", today.format("%Y-%m")),
            &serde_json::to_string(&cached).unwrap(),
            Duration::from_secs(60),
        );

        let calendar = client.get_price_calendar("12345", 3).await.unwrap();
        assert_eq!(
            calendar.days[0].unavailability_reason,
            Some(UnavailabilityReason::PastDate)
        );
        assert_eq!(
            calendar.days[1].unavailability_reason,
            Some(UnavailabilityReason::Unknown)
        );
    }

    #[tokio::test]
    async fn get_host_profile_returns_cached_profile() {
        let (client, cache) = make_client();
        let expected = make_host_profile("Test Host");
        cache.set(
            "gql:host:12345",
            &serde_json::to_string(&expected).unwrap(),
            Duration::from_secs(60),
        );
        let result = client.get_host_profile("12345").await.unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&expected).unwrap(),
        );
    }

    #[test]
    fn graphql_errors_detail_summarises_the_validation_error_capture() {
        let json = crate::test_helpers::fixture_json("graphql/StaysSearch.validation_error.json");
        assert_eq!(
            graphql_errors_detail(&json).as_deref(),
            Some("Sorry, something went wrong. [ValidationError]")
        );
    }

    #[test]
    fn graphql_errors_detail_ignores_successful_captures() {
        let json = crate::test_helpers::fixture_json("graphql/StaysPdpReviewsQuery.response.json");
        assert!(graphql_errors_detail(&json).is_none());
        assert!(graphql_errors_detail(&serde_json::json!({"errors": [], "data": {}})).is_none());
    }

    #[test]
    fn graphql_errors_detail_caps_upstream_text() {
        let long = "x".repeat(MAX_UPSTREAM_MESSAGE_CHARS + 50);
        let json = serde_json::json!({
            "errors": [
                {"message": long},
                {"message": "second", "extensions": {"code": "E2"}},
                {"message": "third"},
                {"message": "fourth"}
            ]
        });
        let detail = graphql_errors_detail(&json).unwrap();
        let parts: Vec<&str> = detail.split("; ").collect();
        assert_eq!(parts.len(), 3, "{detail}");
        assert_eq!(parts[0].chars().count(), MAX_UPSTREAM_MESSAGE_CHARS);
        assert_eq!(parts[1], "second [E2]");
        assert!(!detail.contains("fourth"), "{detail}");
    }

    #[test]
    fn require_root_accepts_the_2026_09_captures() {
        for rel in [
            "graphql/StaysPdpSections.apartment.response.json",
            "graphql/StaysPdpSections.hotel.response.json",
        ] {
            let json = crate::test_helpers::fixture_json(rel);
            assert!(
                require_root(&json, "StaysPdpSections", PDP_SECTIONS_ROOT).is_ok(),
                "{rel}"
            );
        }
        let calendar =
            crate::test_helpers::fixture_json("graphql/PdpAvailabilityCalendar.response.json");
        assert!(require_root(&calendar, "PdpAvailabilityCalendar", CALENDAR_ROOT).is_ok());
    }

    #[test]
    fn require_root_reports_the_config_key_to_update() {
        let err = require_root(
            &serde_json::json!({"data": {"merlin": null}}),
            "PdpAvailabilityCalendar",
            CALENDAR_ROOT,
        )
        .unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamSchema { .. }),
            "got {err:?}"
        );
        assert!(
            err.to_string()
                .contains("scraper.graphql_hashes.pdp_availability_calendar"),
            "{err}"
        );
    }

    #[test]
    fn review_cursor_must_be_a_numeric_offset() {
        assert_eq!(parse_review_cursor(None).unwrap(), 0);
        assert_eq!(parse_review_cursor(Some("48")).unwrap(), 48);
        assert!(matches!(
            parse_review_cursor(Some("abc")),
            Err(AirbnbError::InvalidParams { .. })
        ));
        assert!(matches!(
            parse_review_cursor(Some("-1")),
            Err(AirbnbError::InvalidParams { .. })
        ));
    }

    #[test]
    fn calendar_variables_match_the_2026_09_web_capture() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid date");
        assert_eq!(
            build_calendar_variables("38817969", 12, anchor),
            serde_json::json!({"request": {
                "count": 12,
                "listingId": "38817969",
                "month": 9,
                "year": 2026,
                "returnPropertyLevelCalendarIfApplicable": false
            }})
        );
    }

    pub(super) async fn mount_api_key(server: &wiremock::MockServer) {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"<script>window.__config = {"api_config":{"key":"testkey123"}}</script>"#,
            ))
            .mount(server)
            .await;
    }

    pub(super) fn client_for(
        server: &wiremock::MockServer,
        currency: &str,
        locale: &str,
    ) -> AirbnbGraphQLClient {
        let config = ScraperConfig {
            base_url: server.uri(),
            rate_limit_per_second: 100.0,
            max_retries: 0,
            currency: currency.to_string(),
            locale: locale.to_string(),
            ..ScraperConfig::default()
        };
        let cache: Arc<dyn ListingCache> = Arc::new(MemoryCache::new(64));
        let api_keys = Arc::new(ApiKeyManager::new(
            Client::new(),
            server.uri(),
            60,
            Arc::new(RateLimiter::new(100.0)),
        ));
        AirbnbGraphQLClient::new(
            &config,
            CacheConfig::default(),
            cache,
            api_keys,
            Arc::new(RateLimiter::new(100.0)),
        )
        .expect("graphql client constructs")
    }

    fn query_pairs(request: &wiremock::Request) -> Vec<(String, String)> {
        request
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[tokio::test]
    async fn graphql_get_and_post_pin_configured_currency_and_locale() {
        let server = wiremock::MockServer::start().await;
        mount_api_key(&server).await;
        wiremock::Mock::given(wiremock::matchers::path_regex("^/api/v3/.*"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": null})),
            )
            .mount(&server)
            .await;
        let client = client_for(&server, "EUR", "fr");
        let _ = client.get_listing_detail("12345").await;
        let params = SearchParams {
            location: "Lyon".into(),
            ..Default::default()
        };
        let _ = client.search_listings(&params).await;

        let requests = server
            .received_requests()
            .await
            .expect("request recording is on");
        let api_calls: Vec<&wiremock::Request> = requests
            .iter()
            .filter(|r| r.url.path().starts_with("/api/v3/"))
            .collect();
        assert_eq!(api_calls.len(), 2, "one GET (detail) and one POST (search)");
        for request in api_calls {
            let pairs = query_pairs(request);
            assert!(
                pairs.contains(&("currency".into(), "EUR".into())),
                "{} {pairs:?}",
                request.method
            );
            assert!(
                pairs.contains(&("locale".into(), "fr".into())),
                "{} {pairs:?}",
                request.method
            );
            assert_eq!(
                request
                    .headers
                    .get("accept-language")
                    .and_then(|v| v.to_str().ok()),
                Some("fr,en;q=0.8"),
                "{}",
                request.method
            );
        }
    }

    #[tokio::test]
    async fn unlabelled_detail_and_calendar_get_the_pinned_currency() {
        let server = wiremock::MockServer::start().await;
        mount_api_key(&server).await;
        wiremock::Mock::given(wiremock::matchers::path_regex(
            "^/api/v3/StaysPdpSections/.*",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(crate::test_helpers::fixture_json("p1b/pdp_hotel.json")),
        )
        .mount(&server)
        .await;
        wiremock::Mock::given(wiremock::matchers::path_regex(
            "^/api/v3/PdpAvailabilityCalendar/.*",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(crate::test_helpers::fixture_json("p1b/calendar.json")),
        )
        .mount(&server)
        .await;
        let client = client_for(&server, "EUR", "fr");
        let detail = client
            .get_listing_detail("1257736932578886647")
            .await
            .unwrap();
        assert_eq!(detail.currency, "\u{20ac}");
        let calendar = client.get_price_calendar("38817969", 2).await.unwrap();
        assert_eq!(calendar.currency, "\u{20ac}");
    }

    #[tokio::test]
    async fn detail_and_host_profile_share_one_pdp_request() {
        let server = wiremock::MockServer::start().await;
        mount_api_key(&server).await;
        wiremock::Mock::given(wiremock::matchers::path_regex(
            "^/api/v3/StaysPdpSections/.*",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(crate::test_helpers::fixture_json("p1b/pdp_apartment.json")),
        )
        .expect(2)
        .mount(&server)
        .await;
        let client = client_for(&server, "USD", "en");
        // Detail first, then host: one request.
        client.get_listing_detail("38817969").await.unwrap();
        assert_eq!(
            client.get_host_profile("38817969").await.unwrap().name,
            "Host A"
        );
        // Host first, then detail: one request for another listing id.
        assert_eq!(
            client.get_host_profile("12345").await.unwrap().name,
            "Host A"
        );
        assert_eq!(
            client.get_listing_detail("12345").await.unwrap().name,
            "Charming apartment - Lyon center"
        );
    }
}
