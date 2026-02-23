use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::RwLock;
use tracing::debug;

use crate::error::{AirbnbError, Result};

/// Shared API key manager for Airbnb's internal API.
///
/// Fetches the API key from the Airbnb homepage and caches it with a configurable TTL.
/// Used by both the HTML scraper and GraphQL client.
pub struct ApiKeyManager {
    http: Client,
    base_url: String,
    cache_ttl: Duration,
    cached_key: RwLock<Option<(String, Instant)>>,
}

impl ApiKeyManager {
    pub fn new(http: Client, base_url: String, cache_secs: u64) -> Self {
        Self {
            http,
            base_url,
            cache_ttl: Duration::from_secs(cache_secs),
            cached_key: RwLock::new(None),
        }
    }

    /// Get the Airbnb API key, fetching it from the homepage if not cached.
    pub async fn get_api_key(&self) -> Result<String> {
        // Check cached key
        {
            let guard = self.cached_key.read().await;
            if let Some((ref key, fetched_at)) = *guard
                && fetched_at.elapsed() < self.cache_ttl
            {
                return Ok(key.clone());
            }
        }

        // Fetch fresh key from Airbnb homepage
        debug!("Fetching Airbnb API key from homepage");

        let response = self
            .http
            .get(&self.base_url)
            .send()
            .await
            .map_err(AirbnbError::Http)?;
        let html = response.text().await.map_err(AirbnbError::Http)?;

        let key = extract_api_key(&html).ok_or_else(|| AirbnbError::Parse {
            reason: "could not extract API key from Airbnb homepage".into(),
        })?;

        // Cache it
        {
            let mut guard = self.cached_key.write().await;
            *guard = Some((key.clone(), Instant::now()));
        }

        Ok(key)
    }
}

/// Extract the Airbnb API key from the homepage HTML.
/// The key is embedded in `"api_config":{"key":"<KEY>"`.
pub fn extract_api_key(html: &str) -> Option<String> {
    let marker = "\"api_config\":{\"key\":\"";
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find('"')?;
    let key = &rest[..end];
    if key.is_empty() {
        return None;
    }
    Some(key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_api_key_from_html() {
        let html = r#"<script>window.__config = {"api_config":{"key":"d306zoyjsyarp7ifhu67rjxn52tv0t20"}}</script>"#;
        let key = extract_api_key(html).unwrap();
        assert_eq!(key, "d306zoyjsyarp7ifhu67rjxn52tv0t20");
    }

    #[test]
    fn extract_api_key_missing() {
        let html = "<html><body>No config here</body></html>";
        assert!(extract_api_key(html).is_none());
    }

    #[tokio::test]
    async fn api_key_cached_after_first_fetch() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"<script>window.__config = {"api_config":{"key":"testkey123"}}</script>"#,
            ))
            .expect(1) // Only 1 HTTP request should be made
            .mount(&mock_server)
            .await;

        let http = reqwest::Client::new();
        let mgr = ApiKeyManager::new(http, mock_server.uri(), 3600);
        let key1 = mgr.get_api_key().await.unwrap();
        let key2 = mgr.get_api_key().await.unwrap();
        assert_eq!(key1, "testkey123");
        assert_eq!(key2, "testkey123");
        // wiremock verifies expect(1) on drop
    }

    #[tokio::test]
    async fn api_key_missing_returns_error() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("<html>No config here</html>"),
            )
            .mount(&mock_server)
            .await;

        let http = reqwest::Client::new();
        let mgr = ApiKeyManager::new(http, mock_server.uri(), 3600);
        let result = mgr.get_api_key().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("could not extract API key"));
    }

    #[test]
    fn extract_api_key_empty_value() {
        let html = r#"{"api_config":{"key":""}}"#;
        assert!(extract_api_key(html).is_none());
    }
}
