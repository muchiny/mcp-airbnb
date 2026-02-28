use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tracing::{debug, warn};
use url::Url;

use crate::adapters::scraper::calendar_parser;
use crate::adapters::scraper::detail_parser;
use crate::adapters::scraper::rate_limiter::RateLimiter;
use crate::adapters::scraper::review_parser;
use crate::adapters::scraper::search_parser;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::{CacheConfig, ScraperConfig};
use crate::domain::analytics::{self, HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

pub struct AirbnbScraper {
    http: Client,
    rate_limiter: RateLimiter,
    cache: Arc<dyn ListingCache>,
    config: ScraperConfig,
    cache_config: CacheConfig,
    #[allow(dead_code)] // Kept for CompositeClient construction symmetry
    api_key_manager: Arc<ApiKeyManager>,
}

impl AirbnbScraper {
    pub fn new(
        config: ScraperConfig,
        cache_config: CacheConfig,
        cache: Arc<dyn ListingCache>,
        api_key_manager: Arc<ApiKeyManager>,
    ) -> std::result::Result<Self, reqwest::Error> {
        let http = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .cookie_store(true)
            .build()?;

        let rate_limiter = RateLimiter::new(config.rate_limit_per_second);

        Ok(Self {
            http,
            rate_limiter,
            cache,
            config,
            cache_config,
            api_key_manager,
        })
    }

    async fn fetch_html(&self, url: &str) -> Result<String> {
        self.rate_limiter.wait().await;

        debug!(url, "Fetching page");

        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let delay = Duration::from_secs(u64::from(attempt) * 2);
                debug!(attempt, delay_secs = delay.as_secs(), "Retrying request");
                tokio::time::sleep(delay).await;
                self.rate_limiter.wait().await;
            }

            match self.http.get(url).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return response.text().await.map_err(AirbnbError::Http);
                    }
                    if status.as_u16() == 429 {
                        warn!("Rate limited by Airbnb (429)");
                        last_error = Some(AirbnbError::RateLimited);
                        continue;
                    }
                    if status.as_u16() == 404 {
                        // Extract listing ID from URL if present
                        if let Some(id) = url
                            .split("/rooms/")
                            .nth(1)
                            .and_then(|s| s.split('?').next())
                            .map(String::from)
                        {
                            return Err(AirbnbError::ListingNotFound { id });
                        }
                        return Err(AirbnbError::Parse {
                            reason: format!("page not found (404): {url}"),
                        });
                    }
                    last_error = Some(AirbnbError::Parse {
                        reason: format!("HTTP {status} for {url}"),
                    });
                }
                Err(e) => {
                    warn!(error = %e, attempt, "HTTP request failed");
                    last_error = Some(AirbnbError::Http(e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| AirbnbError::Parse {
            reason: "all retries exhausted".into(),
        }))
    }
}

#[async_trait]
impl AirbnbClient for AirbnbScraper {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        params.validate()?;

        let cache_key = format!("search:{}", build_search_cache_key(params));
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(result) = serde_json::from_str::<SearchResult>(&cached)
        {
            debug!("Cache hit for search");
            return Ok(result);
        }

        let url = build_search_url(&self.config.base_url, params);
        let html = self.fetch_html(&url).await?;
        let result = search_parser::parse_search_results(&html, &self.config.base_url)?;

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
        let cache_key = format!("detail:{id}");
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(detail) = serde_json::from_str::<ListingDetail>(&cached)
        {
            debug!(id, "Cache hit for listing detail");
            return Ok(detail);
        }

        let url = format!("{}/rooms/{id}", self.config.base_url);
        let html = self.fetch_html(&url).await?;
        let detail = detail_parser::parse_listing_detail(&html, id, &self.config.base_url)?;

        if let Ok(json) = serde_json::to_string(&detail) {
            self.cache.set(
                &cache_key,
                &json,
                Duration::from_secs(self.cache_config.detail_ttl_secs),
            );
        }

        Ok(detail)
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
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
        let html = self.fetch_html(&url).await?;
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
        let cache_key = format!("calendar:{id}:m={months}");
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(calendar) = serde_json::from_str::<PriceCalendar>(&cached)
        {
            debug!(id, "Cache hit for calendar");
            return Ok(calendar);
        }

        let mut parsed = Url::parse(&format!("{}/rooms/{id}", self.config.base_url))?;
        parsed
            .query_pairs_mut()
            .append_pair("calendar_months", &months.to_string());
        let url = parsed.to_string();
        let html = self.fetch_html(&url).await?;
        let calendar = calendar_parser::parse_price_calendar(&html, id)?;

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
        let cache_key = format!("host:{listing_id}");
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(profile) = serde_json::from_str::<HostProfile>(&cached)
        {
            debug!(listing_id, "Cache hit for host profile");
            return Ok(profile);
        }

        let url = format!("{}/rooms/{listing_id}", self.config.base_url);
        let html = self.fetch_html(&url).await?;
        let profile = detail_parser::parse_host_profile(&html)?;

        if let Ok(json) = serde_json::to_string(&profile) {
            self.cache.set(
                &cache_key,
                &json,
                Duration::from_secs(self.cache_config.host_profile_ttl_secs),
            );
        }

        Ok(profile)
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

fn build_search_url(base_url: &str, params: &SearchParams) -> String {
    let encoded_location = params.location.replace(' ', "-");
    let base = format!("{base_url}/s/{encoded_location}/homes");

    let query_pairs = params.to_query_pairs();
    if query_pairs.is_empty() {
        return base;
    }

    // Use url crate for proper encoding of query parameters
    if let Ok(mut parsed) = Url::parse(&base) {
        {
            let mut qp = parsed.query_pairs_mut();
            for (k, v) in &query_pairs {
                qp.append_pair(k, v);
            }
        }
        parsed.to_string()
    } else {
        // Fallback: manual construction if base URL can't be parsed
        let encoded: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&query_pairs)
            .finish();
        format!("{base}?{encoded}")
    }
}

fn build_search_cache_key(params: &SearchParams) -> String {
    let mut key = params.location.to_lowercase();
    if let Some(ref checkin) = params.checkin {
        key.push_str(&format!(":ci={checkin}"));
    }
    if let Some(ref checkout) = params.checkout {
        key.push_str(&format!(":co={checkout}"));
    }
    if let Some(adults) = params.adults {
        key.push_str(&format!(":a={adults}"));
    }
    if let Some(children) = params.children {
        key.push_str(&format!(":ch={children}"));
    }
    if let Some(infants) = params.infants {
        key.push_str(&format!(":inf={infants}"));
    }
    if let Some(pets) = params.pets {
        key.push_str(&format!(":p={pets}"));
    }
    if let Some(min_price) = params.min_price {
        key.push_str(&format!(":min={min_price}"));
    }
    if let Some(max_price) = params.max_price {
        key.push_str(&format!(":max={max_price}"));
    }
    if let Some(ref property_type) = params.property_type {
        key.push_str(&format!(":pt={}", property_type.to_lowercase()));
    }
    if let Some(ref cursor) = params.cursor {
        key.push_str(&format!(":cur={cursor}"));
    }
    key
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
        let url = build_search_url("https://www.airbnb.com", &params);
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
        let url = build_search_url("https://www.airbnb.com", &params);
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
    fn build_search_cache_key_location_only() {
        let params = base_params();
        let key = build_search_cache_key(&params);
        assert_eq!(key, "paris");
    }

    #[test]
    fn build_search_cache_key_with_dates() {
        let mut params = base_params();
        params.checkin = Some("2025-06-01".into());
        params.checkout = Some("2025-06-05".into());
        let key = build_search_cache_key(&params);
        assert!(key.contains(":ci=2025-06-01"));
        assert!(key.contains(":co=2025-06-05"));
    }

    #[test]
    fn build_search_cache_key_with_cursor() {
        let mut params = base_params();
        params.cursor = Some("page2".into());
        let key = build_search_cache_key(&params);
        assert!(key.contains(":cur=page2"));
    }

    #[test]
    fn build_search_url_with_price_filters() {
        let mut params = base_params();
        params.min_price = Some(50);
        params.max_price = Some(200);
        let url = build_search_url("https://www.airbnb.com", &params);
        assert!(url.contains("price_min=50"));
        assert!(url.contains("price_max=200"));
    }

    #[test]
    fn cache_key_includes_all_params() {
        let mut params = base_params();
        params.children = Some(1);
        params.infants = Some(1);
        params.pets = Some(1);
        params.min_price = Some(50);
        params.max_price = Some(200);
        params.property_type = Some("Entire home".into());
        let key = build_search_cache_key(&params);
        assert!(key.contains(":ch=1"));
        assert!(key.contains(":inf=1"));
        assert!(key.contains(":p=1"));
        assert!(key.contains(":min=50"));
        assert!(key.contains(":max=200"));
        assert!(key.contains(":pt=entire home"));
    }

    #[test]
    fn build_search_url_with_property_type() {
        let mut params = base_params();
        params.property_type = Some("Entire home".into());
        let url = build_search_url("https://www.airbnb.com", &params);
        assert!(
            url.contains("property_type=Entire+home")
                || url.contains("property_type=Entire%20home")
        );
    }

    #[test]
    fn build_search_url_encodes_special_chars() {
        let mut params = base_params();
        params.cursor = Some("abc&def=123".into());
        let url = build_search_url("https://www.airbnb.com", &params);
        // The cursor value should be properly encoded, not breaking the URL
        assert!(!url.contains("cursor=abc&def=123"));
        assert!(url.contains("cursor=abc%26def%3D123") || url.contains("cursor=abc%26def=123"));
    }

    #[test]
    fn build_search_url_fallback_encodes_special_chars() {
        // Non-absolute URL triggers Url::parse failure, exercising the fallback path
        let mut params = base_params();
        params.cursor = Some("abc&def=123".into());
        let url = build_search_url("not-a-valid-url", &params);
        // The fallback must still URL-encode special characters
        assert!(!url.contains("cursor=abc&def=123"));
        assert!(url.contains("cursor=abc%26def%3D123"));
    }
}
