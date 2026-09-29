use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;
use url::Url;

use crate::adapters::http::{
    MAX_BODY_BYTES, RetryPolicy, http_client_builder, read_body_capped, send_with_policy,
};
use crate::adapters::price;
use crate::adapters::rate_limiter::RateLimiter;
use crate::adapters::request_locale;
use crate::adapters::scraper::calendar_parser;
use crate::adapters::scraper::detail_parser;
use crate::adapters::scraper::review_parser;
use crate::adapters::scraper::search_parser;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::{CacheConfig, ScraperConfig};
use crate::domain::analytics::{self, HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::listing_id::validate_listing_id;
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

/// Context label for HTML fetches in logs and errors (no caller input).
const HTML_CONTEXT: &str = "Airbnb HTML page";

pub struct AirbnbScraper {
    http: Client,
    rate_limiter: Arc<RateLimiter>,
    retry: RetryPolicy,
    cache: Arc<dyn ListingCache>,
    config: ScraperConfig,
    cache_config: CacheConfig,
    #[allow(dead_code)] // The I3 constructor keeps this parameter; HTML pages need no API key.
    api_key_manager: Arc<ApiKeyManager>,
}

impl AirbnbScraper {
    /// `rate_limiter` is the process-wide limiter shared with the GraphQL
    /// client and the API-key manager (I3).
    pub fn new(
        config: ScraperConfig,
        cache_config: CacheConfig,
        cache: Arc<dyn ListingCache>,
        api_key_manager: Arc<ApiKeyManager>,
        rate_limiter: Arc<RateLimiter>,
    ) -> std::result::Result<Self, reqwest::Error> {
        let http = http_client_builder(&config).cookie_store(true).build()?;
        let retry = RetryPolicy::from_config(&config);

        Ok(Self {
            http,
            rate_limiter,
            retry,
            cache,
            config,
            cache_config,
            api_key_manager,
        })
    }

    /// GET an Airbnb HTML page through the shared limiter and retry policy,
    /// with the pinned currency and locale (P1b, I4).
    ///
    /// `listing_id` turns a 404 into `ListingNotFound`; search pages pass `None`.
    async fn fetch_html(&self, url: &str, listing_id: Option<&str>) -> Result<String> {
        // Pin currency and locale like the GraphQL client does (P1b, NET-10).
        let mut pinned = Url::parse(url)?;
        request_locale::pin_currency_and_locale(
            &mut pinned,
            &self.config.currency,
            &self.config.locale,
        );
        debug!(url = %pinned, "Fetching page");
        let request = self.http.get(pinned.as_str()).header(
            "Accept-Language",
            request_locale::accept_language(&self.config.locale),
        );
        match send_with_policy(&request, &self.rate_limiter, &self.retry, HTML_CONTEXT).await {
            Ok(response) => read_body_capped(response, MAX_BODY_BYTES).await,
            Err(AirbnbError::UpstreamStatus { status: 404, .. }) => Err(match listing_id {
                Some(id) => AirbnbError::ListingNotFound { id: id.to_string() },
                None => AirbnbError::UpstreamStatus {
                    status: 404,
                    context: HTML_CONTEXT.to_string(),
                },
            }),
            Err(err) => Err(err),
        }
    }

    fn cached<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.cache
            .get(key)
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn cache_json<T: serde::Serialize>(&self, key: &str, value: &T, ttl_secs: u64) {
        if let Ok(json) = serde_json::to_string(value) {
            self.cache.set(key, &json, Duration::from_secs(ttl_secs));
        }
    }

    /// Fetch `/rooms/{id}` once, parse it once, and cache the detail and the
    /// host profile it contains.
    async fn fetch_listing_page(&self, id: &str) -> Result<detail_parser::ListingPage> {
        let url = format!("{}/rooms/{id}", self.config.base_url);
        let html = self.fetch_html(&url, Some(id)).await?;
        let mut page = detail_parser::parse_listing_page(&html, id, &self.config.base_url);
        if let Ok(detail) = page.detail.as_mut() {
            price::fill_currency(
                &mut detail.currency,
                &price::currency_symbol(&self.config.currency),
            );
            self.cache_json(
                &format!("detail:{id}"),
                &*detail,
                self.cache_config.detail_ttl_secs,
            );
        }
        if let Ok(profile) = page.host.as_ref() {
            self.cache_json(
                &format!("host:{id}"),
                profile,
                self.cache_config.host_profile_ttl_secs,
            );
        }
        Ok(page)
    }
}

#[async_trait]
impl AirbnbClient for AirbnbScraper {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        params.validate()?;

        let cache_key = format!("search:{}", params.cache_key());
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(result) = serde_json::from_str::<SearchResult>(&cached)
        {
            debug!("Cache hit for search");
            return Ok(result);
        }

        let url = build_search_url(&self.config.base_url, params)?;
        let html = self.fetch_html(&url, None).await?;
        let mut result = search_parser::parse_search_results(&html, &self.config.base_url)?;
        price::fill_search_currency(&mut result, &price::currency_symbol(&self.config.currency));

        if let Ok(json) = serde_json::to_string(&result) {
            self.cache.set(
                &cache_key,
                &json,
                Duration::from_secs(self.cache_config.search_ttl_secs),
            );
        }

        Ok(result)
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        validate_listing_id(id)?;
        if let Some(detail) = self.cached::<ListingDetail>(&format!("detail:{id}")) {
            debug!(id, "Cache hit for listing detail");
            return Ok(detail);
        }
        self.fetch_listing_page(id).await?.detail
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        validate_listing_id(id)?;
        let cache_key = format!("reviews:{id}:{}", cursor.unwrap_or("first"));
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(page) = serde_json::from_str::<ReviewsPage>(&cached)
        {
            debug!(id, "Cache hit for reviews");
            return Ok(page);
        }

        let base = format!("{}/rooms/{id}", self.config.base_url);
        let url = if let Some(c) = cursor {
            let mut parsed = Url::parse(&base)?;
            parsed.query_pairs_mut().append_pair("review_cursor", c);
            parsed.to_string()
        } else {
            base
        };
        let html = self.fetch_html(&url, Some(id)).await?;
        let page = review_parser::parse_reviews(&html, id)?;

        if let Ok(json) = serde_json::to_string(&page) {
            self.cache.set(
                &cache_key,
                &json,
                Duration::from_secs(self.cache_config.reviews_ttl_secs),
            );
        }

        Ok(page)
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        validate_listing_id(id)?;
        // Same reference date as the GraphQL client: operator-local "today".
        let today = chrono::Local::now().date_naive();
        let cache_key = format!("calendar:{id}:{}:m={months}", today.format("%Y-%m"));
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(mut calendar) = serde_json::from_str::<PriceCalendar>(&cached)
        {
            debug!(id, "Cache hit for calendar");
            calendar.classify_past_days(today);
            return Ok(calendar);
        }

        let mut parsed = Url::parse(&format!("{}/rooms/{id}", self.config.base_url))?;
        parsed
            .query_pairs_mut()
            .append_pair("calendar_months", &months.to_string());
        let url = parsed.to_string();
        let html = self.fetch_html(&url, Some(id)).await?;
        let mut calendar = calendar_parser::parse_price_calendar(&html, id)?;
        price::fill_currency(
            &mut calendar.currency,
            &price::currency_symbol(&self.config.currency),
        );
        calendar.classify_past_days(today);

        if let Ok(json) = serde_json::to_string(&calendar) {
            self.cache.set(
                &cache_key,
                &json,
                Duration::from_secs(self.cache_config.calendar_ttl_secs),
            );
        }

        Ok(calendar)
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        validate_listing_id(listing_id)?;
        if let Some(profile) = self.cached::<HostProfile>(&format!("host:{listing_id}")) {
            debug!(listing_id, "Cache hit for host profile");
            return Ok(profile);
        }
        self.fetch_listing_page(listing_id).await?.host
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

/// Build the HTML search URL. The free-text location is pushed as ONE
/// percent-encoded path segment, so `/`, `\`, `?`, `#`, `%` and dot-segments
/// in it can never change the path, the query or the fragment.
fn build_search_url(base_url: &str, params: &SearchParams) -> Result<String> {
    let mut url = Url::parse(base_url)?;
    url.path_segments_mut()
        .map_err(|()| {
            AirbnbError::Config(format!("scraper.base_url {base_url:?} cannot carry a path"))
        })?
        .pop_if_empty()
        .push("s")
        .push(&params.location.trim().replace(' ', "-"))
        .push("homes");

    let query_pairs = params.to_query_pairs();
    if !query_pairs.is_empty() {
        let mut query = url.query_pairs_mut();
        for (key, value) in &query_pairs {
            query.append_pair(key, value);
        }
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_url_location_only() {
        let params = SearchParams {
            location: "Paris France".into(),
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
        };
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        assert_eq!(url, "https://www.airbnb.com/s/Paris-France/homes");
    }

    #[test]
    fn build_search_url_with_params() {
        let params = SearchParams {
            location: "Tokyo".into(),
            checkin: Some("2025-07-01".into()),
            checkout: Some("2025-07-05".into()),
            adults: Some(2),
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        assert!(url.contains("checkin=2025-07-01"));
        assert!(url.contains("checkout=2025-07-05"));
        assert!(url.contains("adults=2"));
    }

    fn base_params() -> SearchParams {
        SearchParams {
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
        }
    }

    #[test]
    fn build_search_url_with_price_filters() {
        let mut params = base_params();
        params.min_price = Some(50);
        params.max_price = Some(200);
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        assert!(url.contains("price_min=50"));
        assert!(url.contains("price_max=200"));
    }

    #[test]
    fn build_search_url_with_property_type() {
        let mut params = base_params();
        params.property_type = Some("Entire home".into());
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        assert!(url.contains("room_types%5B%5D=Entire+home%2Fapt"), "{url}");
        assert!(!url.contains("property_type="), "{url}");
    }

    #[test]
    fn build_search_url_encodes_special_chars() {
        let mut params = base_params();
        params.cursor = Some("abc&def=123".into());
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        // The cursor value should be properly encoded, not breaking the URL
        assert!(!url.contains("cursor=abc&def=123"));
        assert!(url.contains("cursor=abc%26def%3D123") || url.contains("cursor=abc%26def=123"));
    }

    #[test]
    fn build_search_url_rejects_an_invalid_base_url() {
        let err = build_search_url("not-a-valid-url", &base_params()).unwrap_err();
        assert!(matches!(err, AirbnbError::Config(_)), "{err}");
    }

    #[test]
    fn build_search_url_trims_and_dashes_the_location() {
        let mut params = base_params();
        params.location = "  Paris France  ".into();
        let url = build_search_url("https://www.airbnb.com", &params).expect("valid base url");
        assert_eq!(url, "https://www.airbnb.com/s/Paris-France/homes");
    }

    pub(super) fn scraper_for(
        server: &wiremock::MockServer,
        currency: &str,
        locale: &str,
    ) -> AirbnbScraper {
        let config = ScraperConfig {
            base_url: server.uri(),
            rate_limit_per_second: 100.0,
            max_retries: 0,
            currency: currency.to_string(),
            locale: locale.to_string(),
            ..ScraperConfig::default()
        };
        let cache: Arc<dyn ListingCache> =
            Arc::new(crate::adapters::cache::memory_cache::MemoryCache::new(64));
        let api_keys = Arc::new(ApiKeyManager::new(
            Client::new(),
            server.uri(),
            60,
            Arc::new(RateLimiter::new(100.0)),
        ));
        AirbnbScraper::new(
            config,
            CacheConfig::default(),
            cache,
            api_keys,
            Arc::new(RateLimiter::new(100.0)),
        )
        .expect("scraper constructs")
    }

    #[tokio::test]
    async fn every_scraper_request_pins_currency_and_locale() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("<html><body><h1>Listing</h1></body></html>"),
            )
            .mount(&server)
            .await;
        let scraper = scraper_for(&server, "EUR", "fr");
        let _ = scraper.get_listing_detail("12345").await;
        let params = SearchParams {
            location: "Lyon".into(),
            ..Default::default()
        };
        let _ = scraper.search_listings(&params).await;
        let _ = scraper.get_price_calendar("12345", 1).await;

        let requests = server
            .received_requests()
            .await
            .expect("request recording is on");
        assert!(requests.len() >= 3, "{} requests", requests.len());
        for request in &requests {
            let pairs: Vec<(String, String)> = request
                .url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            assert!(
                pairs.contains(&("currency".into(), "EUR".into())),
                "{} {pairs:?}",
                request.url
            );
            assert!(
                pairs.contains(&("locale".into(), "fr".into())),
                "{} {pairs:?}",
                request.url
            );
            assert_eq!(
                request
                    .headers
                    .get("accept-language")
                    .and_then(|v| v.to_str().ok()),
                Some("fr,en;q=0.8"),
                "{}",
                request.url
            );
        }
    }

    #[tokio::test]
    async fn unlabelled_search_prices_get_the_pinned_currency() {
        use base64::Engine as _;
        let server = wiremock::MockServer::start().await;
        let item = serde_json::json!({
            "demandStayListing": {
                "id": base64::engine::general_purpose::STANDARD.encode("DemandStayListing:111")
            },
            "title": "Room in Lyon",
            "subtitle": "Nice Room",
            "structuredDisplayPrice": {"primaryLine": {"price": "85", "qualifier": "night"}}
        });
        let payload = serde_json::json!({
            "data": {"presentation": {"staysSearch": {"results": {"searchResults": [item]}}}}
        });
        let html =
            crate::test_helpers::niobe_page("niobeClientData", &[("StaysSearch:{}", &payload)]);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path_regex("^/s/.*"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(html))
            .mount(&server)
            .await;
        let scraper = scraper_for(&server, "EUR", "fr");
        let params = SearchParams {
            location: "Lyon".into(),
            ..Default::default()
        };
        let result = scraper.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].known_price(), Some(85.0));
        assert_eq!(result.listings[0].currency, "\u{20ac}");
    }

    #[tokio::test]
    async fn detail_then_host_profile_fetch_the_listing_page_once() {
        let server = wiremock::MockServer::start().await;
        let payload = crate::test_helpers::fixture_json("p1b/pdp_apartment.json");
        let html = crate::test_helpers::niobe_page(
            "niobeClientData",
            &[("StaysPdpSections:{}", &payload)],
        );
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/rooms/38817969"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(html))
            .expect(1)
            .mount(&server)
            .await;
        let scraper = scraper_for(&server, "USD", "en");
        let detail = scraper.get_listing_detail("38817969").await.unwrap();
        let host = scraper.get_host_profile("38817969").await.unwrap();
        assert_eq!(detail.host_name.as_deref(), Some("Host A"));
        assert_eq!(host.name, "Host A");
    }

    #[test]
    fn build_search_url_keeps_hostile_locations_in_one_segment() {
        let cases = [
            ("../../rooms/12345", "/s/..%2F..%2Frooms%2F12345/homes"),
            (
                "Paris?adults=16&price_max=1#",
                "/s/Paris%3Fadults=16&price_max=1%23/homes",
            ),
            (
                "%2e%2e/%2E%2E/users/show/1",
                "/s/%252e%252e%2F%252E%252E%2Fusers%2Fshow%2F1/homes",
            ),
            ("a\\b", "/s/a%5Cb/homes"),
        ];
        for (location, expected_path) in cases {
            let mut params = base_params();
            params.location = location.into();
            let built =
                build_search_url("https://www.airbnb.com", &params).expect("valid base url");
            let url = Url::parse(&built).expect("valid URL");
            assert_eq!(url.host_str(), Some("www.airbnb.com"), "{location:?}");
            assert_eq!(url.path(), expected_path, "location {location:?}");
            assert_eq!(url.query(), None, "location {location:?} injected a query");
            assert_eq!(
                url.fragment(),
                None,
                "location {location:?} injected a fragment"
            );
        }
    }
}
