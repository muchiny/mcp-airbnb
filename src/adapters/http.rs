//! Shared HTTP plumbing for every adapter that talks to Airbnb.
//!
//! - [`transport_error`] and the `From<url::ParseError>` impl are the only
//!   places that turn `reqwest`/`url` errors into [`AirbnbError`], so the port
//!   error type carries no infrastructure types.
//! - [`http_client_builder`] applies the user agent, the timeouts and a
//!   same-origin redirect policy.
//! - [`send_with_retries`] paces every attempt through the shared
//!   [`RateLimiter`] and applies the retry policy; [`send_with_policy`] also
//!   turns a final non-2xx status into `UpstreamStatus`.
//! - [`read_body_capped`] bounds how much of a response is buffered.

use std::fmt::Write as _;
use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::{ClientBuilder, RequestBuilder, Response, StatusCode};
use tracing::{debug, warn};

use crate::adapters::rate_limiter::RateLimiter;
use crate::config::types::ScraperConfig;
use crate::error::{AirbnbError, Result};

/// Largest response body buffered from Airbnb. Search pages and the homepage
/// weigh about 0.5–1.5 MB, so 16 MiB leaves ample headroom.
pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Maximum number of redirects followed, all on the original origin.
pub const MAX_REDIRECTS: usize = 5;

/// Longest connect timeout, whatever `request_timeout_secs` says.
const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Retry policy for one logical request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Extra attempts after the first one (`scraper.max_retries`).
    pub max_retries: u32,
    /// Retry number `n` (1-based) waits `base_delay * 2^n`, capped at `max_delay`.
    pub base_delay: Duration,
    /// Upper bound of one backoff sleep.
    pub max_delay: Duration,
    /// A 429 is retried only when its `Retry-After` is at most this long.
    pub max_retry_after: Duration,
    /// How long every caller pauses after a 429 that carries no `Retry-After`.
    pub rate_limited_cooldown: Duration,
}

impl RetryPolicy {
    /// Production policy with `max_retries` extra attempts.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(60),
            rate_limited_cooldown: Duration::from_secs(30),
        }
    }

    /// Production policy using `scraper.max_retries`.
    pub fn from_config(config: &ScraperConfig) -> Self {
        Self::new(config.max_retries)
    }

    /// Backoff before retry number `retry` (1-based): 2 s, 4 s, 8 s, … capped.
    pub fn backoff(&self, retry: u32) -> Duration {
        let factor = 1_u32.checked_shl(retry.min(16)).unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// Convert a `reqwest` transport error into [`AirbnbError::Http`].
///
/// The request URL is dropped (its query string can carry caller input) and
/// the failure kind is named so logs and tool errors stay actionable.
pub fn transport_error(err: reqwest::Error) -> AirbnbError {
    let kind = if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connection failed"
    } else if err.is_body() || err.is_decode() {
        "response body error"
    } else {
        "request error"
    };
    let err = err.without_url();
    let mut reason = format!("{kind}: {err}");
    if let Some(source) = std::error::Error::source(&err) {
        let _ = write!(reason, " ({source})");
    }
    AirbnbError::Http { reason }
}

/// URLs are built only from `scraper.base_url` plus fixed paths, so a parse
/// failure is a configuration problem.
impl From<url::ParseError> for AirbnbError {
    fn from(err: url::ParseError) -> Self {
        AirbnbError::Config(format!("invalid URL built from scraper.base_url: {err}"))
    }
}

/// Client builder with the settings every Airbnb client shares: the user
/// agent, the total and connect timeouts, and [`same_origin_redirect_policy`].
/// Callers chain their own options (cookie store, default headers) and build.
pub fn http_client_builder(config: &ScraperConfig) -> ClientBuilder {
    let timeout = Duration::from_secs(config.request_timeout_secs);
    reqwest::Client::builder()
        .user_agent(&config.user_agent)
        .timeout(timeout)
        .connect_timeout(timeout.min(MAX_CONNECT_TIMEOUT))
        .redirect(same_origin_redirect_policy())
}

/// Follow at most [`MAX_REDIRECTS`] redirects, and only while they stay on the
/// origin (scheme, host, port) of the original request. A cross-origin or
/// https-to-http redirect is not followed and its 3xx response is returned
/// as is. The API-key header and the cookie jar therefore never leave the
/// configured origin.
pub fn same_origin_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let same_origin = attempt
            .previous()
            .first()
            .is_some_and(|first| first.origin() == attempt.url().origin());
        if same_origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Read a response body, refusing more than `limit` bytes.
pub async fn read_body_capped(mut response: Response, limit: usize) -> Result<String> {
    let limit_u64 = u64::try_from(limit).unwrap_or(u64::MAX);
    if let Some(declared) = response.content_length()
        && declared > limit_u64
    {
        return Err(AirbnbError::Http {
            reason: format!("response body of {declared} bytes exceeds the {limit}-byte limit"),
        });
    }
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(AirbnbError::Http {
                reason: format!("response body exceeds the {limit}-byte limit"),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Parse `Retry-After` as delta-seconds or as an HTTP date.
pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let remaining = at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    Some(remaining.to_std().unwrap_or(Duration::ZERO))
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT || status.is_server_error()
}

/// Send `request` through the shared `limiter` and the retry `policy`, and
/// return the final response **whatever its status**, except for 429 and
/// transport failures, which become errors. Use it when a non-2xx body still
/// carries information (GraphQL reports a stale persisted query as a 4xx whose
/// JSON body has an `errors` array); otherwise use [`send_with_policy`].
///
/// - 2xx: returned.
/// - 429: retried only when `Retry-After` is present, at most
///   `policy.max_retry_after` long, and retries are left. The shared limiter
///   is paused for that long, so every caller backs off. Otherwise the limiter
///   is paused (for the capped `Retry-After`, or `policy.rate_limited_cooldown`)
///   and [`AirbnbError::RateLimited`] is returned.
/// - 408, 5xx, timeouts and connection failures: retried after
///   [`RetryPolicy::backoff`] while retries are left; the last 408/5xx
///   response is returned, the last transport failure becomes
///   [`AirbnbError::Http`].
/// - any other status (400, 401, 403, 404, 410, 422, a refused redirect…):
///   returned at once, never retried.
///
/// `context` names the request in logs and errors. It must not contain caller input.
pub async fn send_with_retries(
    request: &RequestBuilder,
    limiter: &RateLimiter,
    policy: &RetryPolicy,
    context: &str,
) -> Result<Response> {
    let mut attempt: u32 = 0;
    loop {
        let this_try = request.try_clone().ok_or_else(|| AirbnbError::Http {
            reason: format!("{context}: request could not be built or cannot be replayed"),
        })?;
        let retries_left = attempt < policy.max_retries;
        limiter.wait().await;
        debug!(context, attempt, "sending request");
        match this_try.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    return Ok(response);
                }
                if status == StatusCode::TOO_MANY_REQUESTS {
                    let retry_after = parse_retry_after(response.headers());
                    let retry_now = retries_left
                        && retry_after.is_some_and(|wait| wait <= policy.max_retry_after);
                    let pause = retry_after
                        .unwrap_or(policy.rate_limited_cooldown)
                        .min(policy.max_retry_after);
                    limiter.penalize(pause);
                    if !retry_now {
                        warn!(
                            context,
                            pause_secs = pause.as_secs_f64(),
                            "HTTP 429: pausing all requests and giving up on this one"
                        );
                        return Err(AirbnbError::RateLimited);
                    }
                    warn!(
                        context,
                        pause_secs = pause.as_secs_f64(),
                        "HTTP 429 with Retry-After: pausing all requests, then retrying"
                    );
                } else if retries_left && is_retryable_status(status) {
                    let delay = policy.backoff(attempt + 1);
                    warn!(
                        context,
                        status = status.as_u16(),
                        delay_secs = delay.as_secs_f64(),
                        "retryable HTTP status"
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Ok(response);
                }
            }
            Err(err) => {
                let transient = err.is_timeout() || err.is_connect();
                let mapped = transport_error(err);
                if retries_left && transient {
                    let delay = policy.backoff(attempt + 1);
                    warn!(
                        context,
                        error = %mapped,
                        delay_secs = delay.as_secs_f64(),
                        "transient transport error"
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Err(mapped);
                }
            }
        }
        attempt += 1;
    }
}

/// [`send_with_retries`], then every final non-2xx status (a refused
/// cross-origin redirect included) becomes [`AirbnbError::UpstreamStatus`].
/// Only 2xx responses are returned.
pub async fn send_with_policy(
    request: &RequestBuilder,
    limiter: &RateLimiter,
    policy: &RetryPolicy,
    context: &str,
) -> Result<Response> {
    let response = send_with_retries(request, limiter, policy, context).await?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        Err(AirbnbError::UpstreamStatus {
            status: status.as_u16(),
            context: context.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use wiremock::matchers::{any, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn fast_policy(max_retries: u32) -> RetryPolicy {
        RetryPolicy {
            max_retries,
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(50),
            max_retry_after: Duration::from_secs(2),
            rate_limited_cooldown: Duration::from_millis(300),
        }
    }

    fn fast_limiter() -> RateLimiter {
        RateLimiter::from_interval(Duration::from_millis(1))
    }

    fn client() -> reqwest::Client {
        http_client_builder(&ScraperConfig::default())
            .build()
            .expect("client builds")
    }

    #[tokio::test]
    async fn transport_error_names_the_failure_and_hides_the_url() {
        // Bind then drop a listener: nothing listens on that port any more.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("local addr")
            .port();
        let err = reqwest::Client::new()
            .get(format!(
                "http://127.0.0.1:{port}/rooms/1?location=secret-town"
            ))
            .send()
            .await
            .expect_err("nothing listens on a closed port");
        let mapped = transport_error(err);
        let text = mapped.to_string();
        assert!(matches!(mapped, AirbnbError::Http { .. }), "{text}");
        assert!(text.contains("connection failed"), "{text}");
        assert!(
            !text.contains("secret-town"),
            "URL leaked into the error: {text}"
        );
    }

    #[test]
    fn url_parse_errors_become_config_errors() {
        let err: AirbnbError = url::Url::parse("://missing-scheme")
            .expect_err("invalid URL")
            .into();
        assert!(matches!(err, AirbnbError::Config(_)), "{err}");
        assert!(err.to_string().contains("base_url"), "{err}");
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let policy = RetryPolicy::new(5);
        assert_eq!(policy.backoff(1), Duration::from_secs(2));
        assert_eq!(policy.backoff(2), Duration::from_secs(4));
        assert_eq!(policy.backoff(3), Duration::from_secs(8));
        assert_eq!(policy.backoff(5), Duration::from_secs(30));
        assert_eq!(policy.backoff(64), Duration::from_secs(30));
    }

    #[test]
    fn parse_retry_after_accepts_seconds_and_http_dates() {
        let mut headers = HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);

        headers.insert(RETRY_AFTER, "7".parse().expect("header value"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(7)));

        let in_two_minutes = (chrono::Utc::now() + chrono::TimeDelta::seconds(120))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        headers.insert(RETRY_AFTER, in_two_minutes.parse().expect("header value"));
        let wait = parse_retry_after(&headers).expect("HTTP date parses");
        assert!(
            wait > Duration::from_secs(100) && wait <= Duration::from_secs(120),
            "{wait:?}"
        );

        headers.insert(RETRY_AFTER, "soon".parse().expect("header value"));
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[tokio::test]
    async fn retries_server_errors_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let request = client().get(format!("{}/page", server.uri()));
        let response = send_with_policy(&request, &fast_limiter(), &fast_policy(2), "test page")
            .await
            .expect("third attempt succeeds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn gives_up_after_max_retries() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(500))
            .expect(3)
            .mount(&server)
            .await;

        let request = client().get(server.uri());
        let err = send_with_policy(&request, &fast_limiter(), &fast_policy(2), "test page")
            .await
            .expect_err("always 500");
        assert!(
            matches!(err, AirbnbError::UpstreamStatus { status: 500, .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn send_with_retries_hands_back_a_final_client_error_with_its_body() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"errors":[{"message":"PersistedQueryNotFound"}]}"#),
            )
            .expect(1)
            .mount(&server)
            .await;

        let request = client().get(server.uri());
        let response = send_with_retries(&request, &fast_limiter(), &fast_policy(3), "test page")
            .await
            .expect("a final 4xx is handed back so GraphQL can read its `errors`");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = read_body_capped(response, 1024).await.expect("body");
        assert!(body.contains("PersistedQueryNotFound"), "{body}");
    }

    #[tokio::test]
    async fn send_with_retries_returns_the_last_server_error_after_the_retries() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&server)
            .await;

        let request = client().get(server.uri());
        let response = send_with_retries(&request, &fast_limiter(), &fast_policy(1), "test page")
            .await
            .expect("the last 503 is handed back");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn client_errors_are_never_retried() {
        for status in [400_u16, 401, 403, 404, 410, 422] {
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;

            let request = client().get(server.uri());
            let err = send_with_policy(&request, &fast_limiter(), &fast_policy(3), "test page")
                .await
                .expect_err("4xx is an error");
            assert!(
                matches!(err, AirbnbError::UpstreamStatus { status: s, .. } if s == status),
                "status {status}: {err}"
            );
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn honours_a_short_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let request = client().get(format!("{}/page", server.uri()));
        let start = Instant::now();
        let response = send_with_policy(&request, &fast_limiter(), &fast_policy(1), "test page")
            .await
            .expect("retried after Retry-After");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            start.elapsed() >= Duration::from_millis(900),
            "Retry-After ignored: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn a_429_without_retry_after_is_not_retried_and_pauses_everyone() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429))
            .expect(1)
            .mount(&server)
            .await;

        let limiter = fast_limiter();
        let request = client().get(server.uri());
        let err = send_with_policy(&request, &limiter, &fast_policy(3), "test page")
            .await
            .expect_err("429");
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");

        let start = Instant::now();
        limiter.wait().await;
        assert!(
            start.elapsed() >= Duration::from_millis(250),
            "limiter not paused after 429: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn a_long_retry_after_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&server)
            .await;

        let request = client().get(server.uri());
        let err = send_with_policy(&request, &fast_limiter(), &fast_policy(3), "test page")
            .await
            .expect_err("429 with a long Retry-After");
        assert!(matches!(err, AirbnbError::RateLimited), "{err}");
    }

    #[tokio::test]
    async fn cross_origin_redirect_is_not_followed() {
        let elsewhere = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let origin = MockServer::start().await;
        let target = format!("{}/collect", elsewhere.uri());
        Mock::given(path("/start"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target.as_str()))
            .expect(1)
            .mount(&origin)
            .await;

        let request = client()
            .get(format!("{}/start", origin.uri()))
            .header("X-Airbnb-Api-Key", "secret-key");
        let err = send_with_policy(&request, &fast_limiter(), &fast_policy(0), "test page")
            .await
            .expect_err("redirect to another origin is refused");
        assert!(
            matches!(err, AirbnbError::UpstreamStatus { status: 302, .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn same_origin_redirect_is_followed() {
        let server = MockServer::start().await;
        Mock::given(path("/old"))
            .respond_with(ResponseTemplate::new(301).insert_header("Location", "/new"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/new"))
            .respond_with(ResponseTemplate::new(200).set_body_string("moved"))
            .expect(1)
            .mount(&server)
            .await;

        let request = client().get(format!("{}/old", server.uri()));
        let response = send_with_policy(&request, &fast_limiter(), &fast_policy(0), "test page")
            .await
            .expect("same-origin redirect followed");
        assert_eq!(
            read_body_capped(response, 1024).await.expect("body"),
            "moved"
        );
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(4096)))
            .mount(&server)
            .await;

        let response = client().get(server.uri()).send().await.expect("response");
        let err = read_body_capped(response, 1024)
            .await
            .expect_err("body larger than the cap");
        assert!(err.to_string().contains("1024-byte limit"), "{err}");
    }
}
