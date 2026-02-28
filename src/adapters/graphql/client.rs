use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use reqwest::Client;
use tracing::{debug, trace};
use url::Url;

use crate::adapters::scraper::rate_limiter::RateLimiter;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::{CacheConfig, GraphQLHashes, ScraperConfig};
use crate::domain::analytics::{self, HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

use super::parsers;

pub struct AirbnbGraphQLClient {
    http: Client,
    rate_limiter: RateLimiter,
    cache: Arc<dyn ListingCache>,
    base_url: String,
    hashes: GraphQLHashes,
    cache_config: CacheConfig,
    api_key_manager: Arc<ApiKeyManager>,
}

impl AirbnbGraphQLClient {
    pub fn new(
        config: &ScraperConfig,
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
            base_url: config.base_url.clone(),
            hashes: config.graphql_hashes.clone(),
            cache_config,
            api_key_manager,
        })
    }

    /// Execute a GraphQL GET request with persisted query hash.
    async fn graphql_get(
        &self,
        operation_name: &str,
        hash: &str,
        variables: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let api_key = self.api_key_manager.get_api_key().await?;

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
            .append_pair("locale", "en")
            .append_pair("currency", "USD")
            .append_pair("variables", &variables.to_string())
            .append_pair("extensions", &extensions.to_string());

        self.rate_limiter.wait().await;
        debug!(url = %url, "GraphQL GET request");

        let response = self
            .http
            .get(url.as_str())
            .header("X-Airbnb-Api-Key", &api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .map_err(AirbnbError::Http)?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(AirbnbError::RateLimited);
        }
        if !status.is_success() {
            return Err(AirbnbError::Parse {
                reason: format!("GraphQL {operation_name} returned HTTP {status}"),
            });
        }

        let body = response.text().await.map_err(AirbnbError::Http)?;
        debug!(
            operation = operation_name,
            body_len = body.len(),
            "GraphQL response received"
        );
        trace!(
            operation = operation_name,
            body = %body,
            "GraphQL raw response"
        );

        serde_json::from_str(&body).map_err(|e| AirbnbError::Parse {
            reason: format!("GraphQL {operation_name} JSON parse error: {e}"),
        })
    }

    /// Execute a GraphQL POST request (used for search which requires a body).
    async fn graphql_post(
        &self,
        operation_name: &str,
        hash: &str,
        variables: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let api_key = self.api_key_manager.get_api_key().await?;

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

        let endpoint = format!("{}/api/v3/{operation_name}/{hash}/", self.base_url);

        self.rate_limiter.wait().await;
        debug!(endpoint, "GraphQL POST request");

        let response = self
            .http
            .post(&endpoint)
            .header("X-Airbnb-Api-Key", &api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .json(&body)
            .send()
            .await
            .map_err(AirbnbError::Http)?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(AirbnbError::RateLimited);
        }
        if !status.is_success() {
            return Err(AirbnbError::Parse {
                reason: format!("GraphQL {operation_name} returned HTTP {status}"),
            });
        }

        let resp_body = response.text().await.map_err(AirbnbError::Http)?;
        debug!(
            operation = operation_name,
            body_len = resp_body.len(),
            "GraphQL response received"
        );
        trace!(
            operation = operation_name,
            body = %resp_body,
            "GraphQL raw response"
        );

        serde_json::from_str(&resp_body).map_err(|e| AirbnbError::Parse {
            reason: format!("GraphQL {operation_name} JSON parse error: {e}"),
        })
    }
}

#[async_trait]
impl AirbnbClient for AirbnbGraphQLClient {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        params.validate()?;

        let cache_key = format!("gql:search:{}", params.location.to_lowercase());
        if let Some(cached) = self.cache.get(&cache_key) {
            debug!("Cache hit for GraphQL search");
            if let Ok(result) = serde_json::from_str::<SearchResult>(&cached) {
                return Ok(result);
            }
        }

        let variables = parsers::search::build_search_variables(params);
        let json = self
            .graphql_post("StaysSearch", &self.hashes.stays_search, &variables)
            .await?;
        let result = parsers::search::parse_search_response(&json, &self.base_url)?;

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
        let cache_key = format!("gql:detail:{id}");
        if let Some(cached) = self.cache.get(&cache_key) {
            debug!(id, "Cache hit for GraphQL listing detail");
            if let Ok(detail) = serde_json::from_str::<ListingDetail>(&cached) {
                return Ok(detail);
            }
        }

        let b64 = base64::engine::general_purpose::STANDARD;
        let encoded_id = b64.encode(format!("StayListing:{id}"));
        let demand_id = b64.encode(format!("DemandStayListing:{id}"));

        let variables = serde_json::json!({
            "id": encoded_id,
            "demandStayListingId": demand_id,
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
        });

        let json = self
            .graphql_get(
                "StaysPdpSections",
                &self.hashes.stays_pdp_sections,
                &variables,
            )
            .await?;
        let detail = parsers::detail::parse_detail_response(&json, id, &self.base_url)?;

        if let Ok(serialized) = serde_json::to_string(&detail) {
            self.cache.set(
                &cache_key,
                &serialized,
                Duration::from_secs(self.cache_config.detail_ttl_secs),
            );
        }

        Ok(detail)
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        let cache_key = format!("gql:reviews:{id}:{}", cursor.unwrap_or("first"));
        if let Some(cached) = self.cache.get(&cache_key) {
            debug!(id, "Cache hit for GraphQL reviews");
            if let Ok(page) = serde_json::from_str::<ReviewsPage>(&cached) {
                return Ok(page);
            }
        }

        let offset: u64 = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
        let variables = serde_json::json!({
            "id": id,
            "pdpReviewsRequest": {
                "fieldSelector": "for_p3_translation_only",
                "forPreview": false,
                "limit": 50,
                "offset": offset.to_string(),
                "showingTranslationButton": false,
                "first": 50,
                "sortingPreference": "MOST_RECENT",
                "numberOfAdults": "1",
                "numberOfChildren": "0",
                "numberOfInfants": "0",
                "numberOfPets": "0",
                "after": null,
            }
        });

        let json = self
            .graphql_get(
                "StaysPdpReviewsQuery",
                &self.hashes.stays_pdp_reviews,
                &variables,
            )
            .await?;
        let page = parsers::review::parse_reviews_response(&json, id)?;

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
        let cache_key = format!("gql:calendar:{id}:m={months}");
        if let Some(cached) = self.cache.get(&cache_key) {
            debug!(id, "Cache hit for GraphQL calendar");
            if let Ok(calendar) = serde_json::from_str::<PriceCalendar>(&cached) {
                return Ok(calendar);
            }
        }

        let now = chrono::Utc::now();
        let variables = serde_json::json!({
            "request": {
                "count": months,
                "listingId": id,
                "month": now.format("%m").to_string().parse::<u32>().unwrap_or(1),
                "year": now.format("%Y").to_string().parse::<u32>().unwrap_or(2026),
            }
        });

        let json = self
            .graphql_get(
                "PdpAvailabilityCalendar",
                &self.hashes.pdp_availability_calendar,
                &variables,
            )
            .await?;

        // Reuse the existing calendar parser which already handles GraphQL JSON
        let json_str = json.to_string();
        let calendar =
            crate::adapters::scraper::calendar_parser::parse_price_calendar(&json_str, id)?;

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
        let cache_key = format!("gql:host:{listing_id}");
        if let Some(cached) = self.cache.get(&cache_key) {
            debug!(listing_id, "Cache hit for GraphQL host profile");
            if let Ok(profile) = serde_json::from_str::<HostProfile>(&cached) {
                return Ok(profile);
            }
        }

        // Use PDP sections instead of GetUserProfile (which requires a user_id, not listing_id)
        let b64 = base64::engine::general_purpose::STANDARD;
        let encoded_id = b64.encode(format!("StayListing:{listing_id}"));
        let demand_id = b64.encode(format!("DemandStayListing:{listing_id}"));

        let variables = serde_json::json!({
            "id": encoded_id,
            "demandStayListingId": demand_id,
            "pdpSectionsRequest": {
                "adults": "1",
                "layouts": ["SIDEBAR", "SINGLE_COLUMN"],
                "preview": false,
                "staysBookingMigrationEnabled": false,
                "useNewSectionWrapperApi": false,
            }
        });

        let json = self
            .graphql_get(
                "StaysPdpSections",
                &self.hashes.stays_pdp_sections,
                &variables,
            )
            .await?;
        let profile = parsers::host::parse_host_response(&json)?;

        if let Ok(serialized) = serde_json::to_string(&profile) {
            self.cache.set(
                &cache_key,
                &serialized,
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
