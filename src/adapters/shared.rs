//! Airbnb's public web API key: fetched from the homepage, cached, and shared
//! by every client.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::adapters::http::{MAX_BODY_BYTES, RetryPolicy, read_body_capped, send_with_policy};
use crate::adapters::rate_limiter::RateLimiter;
use crate::error::{AirbnbError, Result};

/// How long a failed key fetch is remembered before the homepage is fetched
/// again, so a broken or blocked homepage is not re-downloaded on every call.
pub const KEY_FAILURE_BACKOFF: Duration = Duration::from_secs(120);

const KEY_CONTEXT: &str = "Airbnb homepage (API key)";

/// Shared API key manager for Airbnb's internal API.
///
/// - Single-flight: concurrent callers wait for the one in-flight fetch.
/// - The homepage fetch goes through the shared [`RateLimiter`], and its HTTP
///   status is checked (429 becomes `RateLimited`, other failures `UpstreamStatus`).
/// - A failed fetch is remembered for [`KEY_FAILURE_BACKOFF`].
/// - [`ApiKeyManager::invalidate`] drops a key that Airbnb rejected (401/403).
pub struct ApiKeyManager {
    http: Client,
    base_url: String,
    cache_ttl: Duration,
    rate_limiter: Arc<RateLimiter>,
    retry: RetryPolicy,
    state: Mutex<KeyState>,
}

#[derive(Default)]
struct KeyState {
    key: Option<(String, Instant)>,
    failure: Option<(KeyFailure, Instant)>,
}

/// What went wrong on the last fetch; replayed while the backoff runs.
#[derive(Debug, Clone)]
enum KeyFailure {
    RateLimited,
    Status(u16),
    Missing,
    Transport(String),
}

impl KeyFailure {
    fn from_error(err: &AirbnbError) -> Self {
        match err {
            AirbnbError::RateLimited => Self::RateLimited,
            AirbnbError::UpstreamStatus { status, .. } => Self::Status(*status),
            AirbnbError::Http { reason } => Self::Transport(reason.clone()),
            _ => Self::Missing,
        }
    }

    fn to_error(&self) -> AirbnbError {
        match self {
            Self::RateLimited => AirbnbError::RateLimited,
            Self::Status(status) => AirbnbError::UpstreamStatus {
                status: *status,
                context: KEY_CONTEXT.to_string(),
            },
            Self::Missing => AirbnbError::UpstreamSchema {
                operation: "ApiKey".to_string(),
                detail: "could not extract API key from Airbnb homepage".to_string(),
            },
            Self::Transport(reason) => AirbnbError::Http {
                reason: reason.clone(),
            },
        }
    }
}

impl ApiKeyManager {
    /// `rate_limiter` is the process-wide limiter built in
    /// `application::build_client`.
    pub fn new(
        http: Client,
        base_url: String,
        cache_secs: u64,
        rate_limiter: Arc<RateLimiter>,
    ) -> Self {
        Self {
            http,
            base_url,
            cache_ttl: Duration::from_secs(cache_secs),
            rate_limiter,
            retry: RetryPolicy::new(0),
            state: Mutex::new(KeyState::default()),
        }
    }

    /// Get the Airbnb API key, fetching it from the homepage when it is not
    /// cached. Concurrent callers share one fetch.
    pub async fn get_api_key(&self) -> Result<String> {
        let mut state = self.state.lock().await;
        if let Some((key, fetched_at)) = &state.key
            && fetched_at.elapsed() < self.cache_ttl
        {
            return Ok(key.clone());
        }
        if let Some((failure, failed_at)) = &state.failure
            && failed_at.elapsed() < KEY_FAILURE_BACKOFF
        {
            debug!("API key fetch failed recently; not retrying yet");
            return Err(failure.to_error());
        }
        match self.fetch_key().await {
            Ok(key) => {
                state.key = Some((key.clone(), Instant::now()));
                state.failure = None;
                Ok(key)
            }
            Err(err) => {
                warn!(error = %err, "could not obtain the Airbnb API key");
                state.failure = Some((KeyFailure::from_error(&err), Instant::now()));
                Err(err)
            }
        }
    }

    /// Forget `rejected_key` after Airbnb answered 401/403 with it, so the next
    /// call fetches a fresh key. A key that a concurrent refresh has already
    /// replaced is kept.
    pub async fn invalidate(&self, rejected_key: &str) {
        let mut state = self.state.lock().await;
        if state
            .key
            .as_ref()
            .is_some_and(|(key, _)| key == rejected_key)
        {
            debug!("dropping the API key Airbnb rejected");
            state.key = None;
        }
    }

    async fn fetch_key(&self) -> Result<String> {
        debug!("Fetching Airbnb API key from homepage");
        let request = self.http.get(&self.base_url);
        let response =
            send_with_policy(&request, &self.rate_limiter, &self.retry, KEY_CONTEXT).await?;
        let html = read_body_capped(response, MAX_BODY_BYTES).await?;
        extract_api_key(&html).ok_or_else(|| KeyFailure::Missing.to_error())
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const KEY_HTML: &str =
        r#"<script>window.__config = {"api_config":{"key":"testkey123"}}</script>"#;

    fn fast_limiter() -> Arc<RateLimiter> {
        Arc::new(RateLimiter::from_interval(Duration::from_millis(1)))
    }

    fn manager(server: &MockServer) -> ApiKeyManager {
        ApiKeyManager::new(Client::new(), server.uri(), 3600, fast_limiter())
    }

    async fn homepage(server: &MockServer, template: ResponseTemplate, expected_calls: u64) {
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(template)
            .expect(expected_calls)
            .mount(server)
            .await;
    }

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

    #[test]
    fn extract_api_key_empty_value() {
        let html = r#"{"api_config":{"key":""}}"#;
        assert!(extract_api_key(html).is_none());
    }

    #[tokio::test]
    async fn api_key_cached_after_first_fetch() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string(KEY_HTML),
            1,
        )
        .await;
        let mgr = manager(&server);
        assert_eq!(mgr.get_api_key().await.unwrap(), "testkey123");
        assert_eq!(mgr.get_api_key().await.unwrap(), "testkey123");
    }

    #[tokio::test]
    async fn api_key_missing_returns_error() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string("<html>No config here</html>"),
            1,
        )
        .await;
        let err = manager(&server).get_api_key().await.unwrap_err();
        assert!(matches!(err, AirbnbError::UpstreamSchema { .. }), "{err}");
        assert!(
            err.to_string().contains("could not extract API key"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_homepage_fetch() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_string(KEY_HTML),
            1,
        )
        .await;
        let mgr = manager(&server);
        let (k1, k2, k3, k4, k5) = tokio::join!(
            mgr.get_api_key(),
            mgr.get_api_key(),
            mgr.get_api_key(),
            mgr.get_api_key(),
            mgr.get_api_key()
        );
        for key in [k1, k2, k3, k4, k5] {
            assert_eq!(key.expect("every caller gets the key"), "testkey123");
        }
    }

    #[tokio::test]
    async fn failed_fetch_is_not_repeated_within_the_backoff() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string("<html>No config here</html>"),
            1,
        )
        .await;
        let mgr = manager(&server);
        assert!(mgr.get_api_key().await.is_err());
        let second = mgr.get_api_key().await.unwrap_err();
        assert!(
            matches!(second, AirbnbError::UpstreamSchema { .. }),
            "{second}"
        );
    }

    #[tokio::test]
    async fn homepage_error_status_is_reported_as_a_status() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(403).set_body_string("blocked"),
            1,
        )
        .await;
        let err = manager(&server).get_api_key().await.unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamStatus { status: 403, .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn homepage_429_is_rate_limited() {
        let server = MockServer::start().await;
        homepage(&server, ResponseTemplate::new(429), 1).await;
        let err = manager(&server).get_api_key().await.unwrap_err();
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn invalidate_forces_a_fresh_fetch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"api_config":{"key":"key-one"}}"#),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string(r#"{"api_config":{"key":"key-two"}}"#),
            1,
        )
        .await;
        let mgr = manager(&server);
        assert_eq!(mgr.get_api_key().await.unwrap(), "key-one");
        mgr.invalidate("key-one").await;
        assert_eq!(mgr.get_api_key().await.unwrap(), "key-two");
    }

    #[tokio::test]
    async fn invalidate_keeps_a_key_that_was_already_replaced() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string(KEY_HTML),
            1,
        )
        .await;
        let mgr = manager(&server);
        assert_eq!(mgr.get_api_key().await.unwrap(), "testkey123");
        mgr.invalidate("some-older-key").await;
        assert_eq!(mgr.get_api_key().await.unwrap(), "testkey123");
    }

    #[tokio::test]
    async fn key_fetch_waits_for_the_shared_rate_limiter() {
        let server = MockServer::start().await;
        homepage(
            &server,
            ResponseTemplate::new(200).set_body_string(KEY_HTML),
            1,
        )
        .await;
        let limiter = Arc::new(RateLimiter::from_interval(Duration::from_millis(300)));
        let mgr = ApiKeyManager::new(Client::new(), server.uri(), 3600, Arc::clone(&limiter));
        limiter.wait().await; // another adapter just used the budget
        let start = std::time::Instant::now();
        mgr.get_api_key().await.unwrap();
        assert!(
            start.elapsed() >= Duration::from_millis(250),
            "key fetch bypassed the limiter: {:?}",
            start.elapsed()
        );
    }
}
