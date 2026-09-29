use std::sync::Arc;

use mcp_airbnb::adapters::cache::memory_cache::MemoryCache;
use mcp_airbnb::adapters::graphql::client::AirbnbGraphQLClient;
use mcp_airbnb::adapters::rate_limiter::RateLimiter;
use mcp_airbnb::adapters::shared::ApiKeyManager;
use mcp_airbnb::config::types::{CacheConfig, ScraperConfig};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::AirbnbError;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use wiremock::matchers::{header, method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fast_graphql_config(base_url: &str) -> ScraperConfig {
    ScraperConfig {
        base_url: base_url.to_string(),
        rate_limit_per_second: 100.0, // fast for tests
        request_timeout_secs: 5,
        max_retries: 0,
        ..Default::default()
    }
}

fn test_cache_config() -> CacheConfig {
    CacheConfig {
        search_ttl_secs: 60,
        detail_ttl_secs: 60,
        reviews_ttl_secs: 60,
        calendar_ttl_secs: 60,
        ..Default::default()
    }
}

fn test_api_key_manager(base_url: &str) -> Arc<ApiKeyManager> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    Arc::new(ApiKeyManager::new(
        http,
        base_url.to_string(),
        86400,
        test_limiter(),
    ))
}

/// Fast pacing so tests do not sleep. Production builds one limiter in
/// `application::build_client`.
fn test_limiter() -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new(100.0))
}

async fn mount_api_key_mock(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<script>window.__config = {"api_config":{"key":"testkey123"}}</script>"#,
        ))
        .mount(server)
        .await;
}

/// A 4-night stay starting 60 days from today: always inside
/// `SearchParams::validate`'s booking horizon (VAL-2).
fn future_stay() -> (String, String) {
    let checkin = chrono::Utc::now().date_naive() + chrono::Days::new(60);
    let checkout = checkin + chrono::Days::new(4);
    (
        checkin.format("%Y-%m-%d").to_string(),
        checkout.format("%Y-%m-%d").to_string(),
    )
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

async fn build_client(server: &MockServer) -> AirbnbGraphQLClient {
    mount_api_key_mock(server).await;
    let cache = Arc::new(MemoryCache::new(100));
    AirbnbGraphQLClient::new(
        &fast_graphql_config(&server.uri()),
        test_cache_config(),
        cache,
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap()
}

async fn build_client_with_cache(
    server: &MockServer,
    cache: Arc<MemoryCache>,
) -> AirbnbGraphQLClient {
    mount_api_key_mock(server).await;
    AirbnbGraphQLClient::new(
        &fast_graphql_config(&server.uri()),
        test_cache_config(),
        cache,
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// JSON fixtures
// ---------------------------------------------------------------------------

fn search_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "staysSearch": {
                    "results": {
                        "searchResults": [{
                            "listing": {
                                "id": "12345",
                                "name": "Cozy Apartment",
                                "city": "Paris",
                                "avgRating": 4.85,
                                "reviewsCount": 42,
                                "isSuperhost": true,
                                "latitude": 48.8566,
                                "longitude": 2.3522
                            },
                            "pricingQuote": {
                                "rate": { "amount": 120.0, "currency": "EUR" }
                            }
                        }],
                        "paginationInfo": {
                            "totalCount": 1,
                            "nextPageCursor": null
                        }
                    }
                }
            }
        }
    })
}

fn empty_search_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "staysSearch": {
                    "results": {
                        "searchResults": [],
                        "paginationInfo": { "totalCount": 0 }
                    }
                }
            }
        }
    })
}

fn detail_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "sections": {
                        "sections": [
                            {
                                "sectionComponentType": "TITLE_DEFAULT",
                                "section": {
                                    "title": "Charming Studio",
                                    "subtitle": "Paris, France"
                                }
                            },
                            {
                                "sectionComponentType": "BOOK_IT_SIDEBAR",
                                "section": {
                                    "structuredDisplayPrice": {
                                        "primaryLine": { "price": "$150" }
                                    },
                                    "maxGuestCapacity": 4
                                }
                            },
                            {
                                "sectionComponentType": "REVIEWS_DEFAULT",
                                "section": {
                                    "overallRating": 4.9,
                                    "overallCount": 55
                                }
                            }
                        ]
                    }
                }
            }
        }
    })
}

fn minimal_detail_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "sections": {
                        "sections": [
                            {
                                "sectionComponentType": "TITLE_DEFAULT",
                                "section": {
                                    "title": "Minimal Place",
                                    "subtitle": "Unknown"
                                }
                            }
                        ]
                    }
                }
            }
        }
    })
}

fn reviews_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "reviews": {
                        "overallRating": 4.85,
                        "reviewsCount": 100,
                        "metadata": { "offset": 0 },
                        "reviews": [
                            {
                                "reviewer": { "firstName": "Alice", "location": "New York" },
                                "createdAt": "2025-01-15",
                                "rating": 5.0,
                                "comments": "Wonderful stay!",
                                "language": "en"
                            },
                            {
                                "reviewer": { "firstName": "Bob" },
                                "createdAt": "2025-01-10",
                                "rating": 4.0,
                                "comments": "Great place but noisy street."
                            }
                        ]
                    }
                }
            }
        }
    })
}

fn empty_reviews_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "reviews": {
                        "reviews": []
                    }
                }
            }
        }
    })
}

fn calendar_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "merlin": {
                "pdpAvailabilityCalendar": {
                    "calendarMonths": [{
                        "month": 3,
                        "year": 2099,
                        "days": [
                            { "calendarDate": "2099-03-01", "available": true, "price": { "amount": 120.0 }, "minNights": 2, "maxNights": 30 },
                            { "calendarDate": "2099-03-02", "available": true, "price": { "amount": 130.0 }, "minNights": 2, "maxNights": 30 },
                            { "calendarDate": "2099-03-03", "available": false, "price": { "amount": 120.0 }, "minNights": 2, "maxNights": 30 }
                        ]
                    }]
                }
            }
        }
    })
}

fn host_sections_response_json() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "sections": {
                        "sections": [{
                            "sectionComponentType": "MEET_YOUR_HOST",
                            "section": {
                                "cardData": {
                                    "name": "Alice",
                                    "userId": "99999",
                                    "isSuperhost": true,
                                    "profilePictureUrl": "https://example.com/alice.jpg"
                                },
                                "about": "Experienced host",
                                "hostDetails": [
                                    "Response rate: 100%",
                                    "Responds within an hour"
                                ],
                                "hostHighlights": [
                                    { "title": "Speaks English and French" }
                                ],
                                "listingsCount": 3
                            }
                        }]
                    }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Search tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_search_parses_response() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .mount(&server)
        .await;

    let result = client.search_listings(&base_params()).await.unwrap();
    assert_eq!(result.listings.len(), 1);
    assert_eq!(result.listings[0].id, "12345");
    assert_eq!(result.listings[0].name, "Cozy Apartment");
    assert!((result.listings[0].price_per_night - 120.0).abs() < 0.01);
    assert_eq!(result.listings[0].is_superhost, Some(true));
}

#[tokio::test]
async fn graphql_search_empty_results() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_search_response_json()))
        .mount(&server)
        .await;

    let result = client.search_listings(&base_params()).await.unwrap();
    assert!(result.listings.is_empty());
    assert_eq!(result.total_count, Some(0));
}

#[tokio::test]
async fn graphql_search_caches_results() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .expect(1) // Only 1 HTTP request; second should hit cache
        .mount(&server)
        .await;

    let r1 = client.search_listings(&base_params()).await.unwrap();
    let r2 = client.search_listings(&base_params()).await.unwrap();
    assert_eq!(r1.listings[0].id, r2.listings[0].id);
    // wiremock verifies expect(1) on drop
}

#[tokio::test]
async fn graphql_search_validates_params() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    // No GraphQL mock needed — validation should fail before HTTP call
    let mut params = base_params();
    params.location = String::new();
    let result = client.search_listings(&params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("location"));
}

#[tokio::test]
async fn graphql_search_different_params_different_cache_keys() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .expect(2) // Should make 2 HTTP calls for different locations
        .mount(&server)
        .await;

    let mut p1 = base_params();
    p1.location = "Paris".into();
    let mut p2 = base_params();
    p2.location = "London".into();

    client.search_listings(&p1).await.unwrap();
    client.search_listings(&p2).await.unwrap();
    // wiremock verifies expect(2)
}

// ---------------------------------------------------------------------------
// Detail tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_detail_parses_response() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response_json()))
        .mount(&server)
        .await;

    let detail = client.get_listing_detail("501").await.unwrap();
    assert_eq!(detail.name, "Charming Studio");
}

#[tokio::test]
async fn graphql_detail_minimal_sections() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(minimal_detail_response_json()))
        .mount(&server)
        .await;

    let detail = client.get_listing_detail("501").await.unwrap();
    assert_eq!(detail.name, "Minimal Place");
    assert!((detail.price_per_night - 0.0).abs() < 0.01);
}

#[tokio::test]
async fn graphql_detail_caches_results() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let d1 = client.get_listing_detail("501").await.unwrap();
    let d2 = client.get_listing_detail("501").await.unwrap();
    assert_eq!(d1.name, d2.name);
}

// ---------------------------------------------------------------------------
// Reviews tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_reviews_parses_response() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reviews_response_json()))
        .mount(&server)
        .await;

    let page = client.get_reviews("501", None).await.unwrap();
    assert_eq!(page.listing_id, "501");
    assert_eq!(page.reviews.len(), 2);
    assert_eq!(page.reviews[0].author, "Alice");
    assert_eq!(page.reviews[0].comment, "Wonderful stay!");

    let summary = page.summary.unwrap();
    assert!((summary.overall_rating - 4.85).abs() < 0.01);
    assert_eq!(summary.total_reviews, 100);
}

#[tokio::test]
async fn graphql_reviews_empty() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_reviews_response_json()))
        .mount(&server)
        .await;

    let page = client.get_reviews("501", None).await.unwrap();
    assert!(page.reviews.is_empty());
    assert!(page.summary.is_none());
}

#[tokio::test]
async fn graphql_reviews_caches_results() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reviews_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let r1 = client.get_reviews("501", None).await.unwrap();
    let r2 = client.get_reviews("501", None).await.unwrap();
    assert_eq!(r1.reviews.len(), r2.reviews.len());
}

#[tokio::test]
async fn graphql_reviews_different_cursor_different_cache_keys() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reviews_response_json()))
        .expect(2) // Two different cache keys
        .mount(&server)
        .await;

    client.get_reviews("501", None).await.unwrap();
    client.get_reviews("501", Some("50")).await.unwrap();
}

// ---------------------------------------------------------------------------
// Calendar tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_calendar_parses_response() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/PdpAvailabilityCalendar/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(calendar_response_json()))
        .mount(&server)
        .await;

    let calendar = client.get_price_calendar("501", 1).await.unwrap();
    assert_eq!(calendar.listing_id, "501");
    assert!(!calendar.days.is_empty());
}

#[tokio::test]
async fn graphql_calendar_caches_results() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/PdpAvailabilityCalendar/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(calendar_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    client.get_price_calendar("501", 1).await.unwrap();
    client.get_price_calendar("501", 1).await.unwrap();
}

// ---------------------------------------------------------------------------
// Host profile tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_host_parses_from_sections() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(host_sections_response_json()))
        .mount(&server)
        .await;

    let host = client.get_host_profile("501").await.unwrap();
    assert_eq!(host.name, "Alice");
    assert_eq!(host.host_id, Some("99999".to_string()));
    assert_eq!(host.is_superhost, Some(true));
    assert_eq!(host.languages, vec!["English", "French"]);
    assert_eq!(host.total_listings, Some(3));
    assert_eq!(host.description, Some("Experienced host".to_string()));
}

#[tokio::test]
async fn graphql_host_missing_section_errors() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    let json = serde_json::json!({
        "data": {
            "presentation": {
                "stayProductDetailPage": {
                    "sections": {
                        "sections": [{
                            "sectionComponentType": "TITLE_DEFAULT",
                            "section": {}
                        }]
                    }
                }
            }
        }
    });

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json))
        .mount(&server)
        .await;

    let result = client.get_host_profile("501").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("host"));
}

// ---------------------------------------------------------------------------
// Delegation tests (neighborhood + occupancy)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_neighborhood_stats_aggregates_search() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .mount(&server)
        .await;

    let stats = client.get_neighborhood_stats(&base_params()).await.unwrap();
    assert_eq!(stats.total_listings, 1);
    assert_eq!(stats.location, "Paris");
}

#[tokio::test]
async fn graphql_occupancy_from_calendar() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/PdpAvailabilityCalendar/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(calendar_response_json()))
        .mount(&server)
        .await;

    let estimate = client.get_occupancy_estimate("501", 1).await.unwrap();
    assert_eq!(estimate.listing_id, "501");
    assert!(estimate.total_days > 0);
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_get_429_returns_rate_limited() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .mount(&server)
        .await;

    let result = client.get_listing_detail("501").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Rate limit"));
}

#[tokio::test]
async fn graphql_post_429_returns_rate_limited() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .mount(&server)
        .await;

    let result = client.search_listings(&base_params()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Rate limit"));
}

#[tokio::test]
async fn graphql_get_500_returns_upstream_status() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let err = client
        .get_listing_detail("501")
        .await
        .expect_err("HTTP 500 must fail");
    assert!(
        matches!(err, AirbnbError::UpstreamStatus { status: 500, .. }),
        "{err}"
    );
}

#[tokio::test]
async fn graphql_invalid_json_returns_parse_error() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not valid json"))
        .mount(&server)
        .await;

    let result = client.get_listing_detail("501").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("JSON parse error"));
}

#[tokio::test]
async fn graphql_malformed_response_structure() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    // Valid JSON but missing expected structure
    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "unexpected": "structure" }
        })))
        .mount(&server)
        .await;

    let err = client.search_listings(&base_params()).await.unwrap_err();
    assert!(
        matches!(err, AirbnbError::UpstreamSchema { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("could not find"), "{err}");
}

#[tokio::test]
async fn graphql_api_key_extraction_failure() {
    let server = MockServer::start().await;

    // Mount homepage that has no API key
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>No config here</html>"))
        .mount(&server)
        .await;

    // Mount a GraphQL endpoint (should not be reached)
    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response_json()))
        .expect(0)
        .mount(&server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let client = AirbnbGraphQLClient::new(
        &fast_graphql_config(&server.uri()),
        test_cache_config(),
        cache,
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap();

    let result = client.get_listing_detail("501").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API key"));
}

// ---------------------------------------------------------------------------
// Upstream drift detection (UpstreamSchema)
// ---------------------------------------------------------------------------

const VALIDATION_ERROR_CAPTURE: &str =
    include_str!("fixtures/airbnb/2026-09/graphql/StaysSearch.validation_error.json");

#[tokio::test]
async fn graphql_errors_payload_is_upstream_schema_and_not_cached() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALIDATION_ERROR_CAPTURE))
        .expect(2) // errors are never cached: the second call hits upstream again
        .mount(&server)
        .await;

    for _ in 0..2 {
        let err = client.search_listings(&base_params()).await.unwrap_err();
        assert!(
            matches!(&err, AirbnbError::UpstreamSchema { operation, .. } if operation == "StaysSearch"),
            "got {err:?}"
        );
        let text = err.to_string();
        assert!(text.contains("[ValidationError]"), "{text}");
        assert!(
            text.contains("scraper.graphql_hashes.stays_search"),
            "{text}"
        );
    }
}

#[tokio::test]
async fn graphql_http_422_is_upstream_schema() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    let err = client.get_reviews("501", None).await.unwrap_err();
    assert!(
        matches!(&err, AirbnbError::UpstreamSchema { operation, .. } if operation == "StaysPdpReviewsQuery"),
        "got {err:?}"
    );
    assert!(
        err.to_string()
            .contains("scraper.graphql_hashes.stays_pdp_reviews"),
        "{err}"
    );
}

#[tokio::test]
async fn graphql_http_400_with_errors_body_keeps_the_upstream_message() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "errors": [{
                "message": "PersistedQueryNotFound",
                "extensions": { "code": "PERSISTED_QUERY_NOT_FOUND" }
            }]
        })))
        .mount(&server)
        .await;

    let err = client.get_listing_detail("501").await.unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, AirbnbError::UpstreamSchema { .. }),
        "got {text}"
    );
    assert!(
        text.contains("PersistedQueryNotFound [PERSISTED_QUERY_NOT_FOUND]"),
        "{text}"
    );
}

#[tokio::test]
async fn graphql_calendar_without_expected_root_is_upstream_schema() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/PdpAvailabilityCalendar/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "merlin": { "pdpAvailabilityCalendar": null } }
        })))
        .mount(&server)
        .await;

    let err = client.get_price_calendar("501", 1).await.unwrap_err();
    assert!(
        matches!(&err, AirbnbError::UpstreamSchema { operation, .. } if operation == "PdpAvailabilityCalendar"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn graphql_requests_mirror_web_client_headers_and_post_url() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response_json()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .and(query_param("operationName", "StaysSearch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    client.get_listing_detail("501").await.unwrap();
    client.search_listings(&base_params()).await.unwrap();

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    let graphql: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path().starts_with("/api/v3/"))
        .collect();
    assert_eq!(graphql.len(), 2);
    for request in graphql {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(
            header("x-airbnb-graphql-platform").as_deref(),
            Some("web"),
            "{}",
            request.url
        );
        assert_eq!(
            header("x-airbnb-graphql-platform-client").as_deref(),
            Some("minimalist-niobe"),
            "{}",
            request.url
        );
    }
    let post = requests
        .iter()
        .find(|r| r.method.as_str() == "POST")
        .expect("a StaysSearch POST was sent");
    assert!(
        !post.url.path().ends_with('/'),
        "the web client posts to /api/v3/StaysSearch/{{hash}} without a trailing slash: {}",
        post.url
    );
}

// ---------------------------------------------------------------------------
// Reviews request contract (2026-09 web client)
// ---------------------------------------------------------------------------

const REVIEWS_CAPTURE: &str =
    include_str!("fixtures/airbnb/2026-09/graphql/StaysPdpReviewsQuery.response.json");

/// Decode the `variables` query parameter of a recorded GraphQL GET request.
fn graphql_variables(request: &wiremock::Request) -> serde_json::Value {
    let raw = request
        .url
        .query_pairs()
        .find(|(key, _)| key == "variables")
        .map(|(_, value)| value.into_owned())
        .expect("GraphQL GET carries a `variables` query parameter");
    serde_json::from_str(&raw).expect("`variables` is JSON")
}

async fn mount_reviews_capture(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REVIEWS_CAPTURE))
        .mount(server)
        .await;
}

fn recorded_reviews_request(requests: &[wiremock::Request]) -> &wiremock::Request {
    requests
        .iter()
        .find(|r| r.url.path().starts_with("/api/v3/StaysPdpReviewsQuery/"))
        .expect("a StaysPdpReviewsQuery request was sent")
}

#[tokio::test]
async fn graphql_reviews_request_matches_2026_09_web_client() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    mount_reviews_capture(&server).await;

    let page = client.get_reviews("38817969", None).await.unwrap();
    assert_eq!(page.reviews.len(), 24);
    assert_eq!(page.next_cursor.as_deref(), Some("24"));

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    let request = recorded_reviews_request(&requests);
    assert!(
        request
            .url
            .path()
            .contains("cfdc3ffbe997a618795fc5a8f9a9b484054ce9be68c8788cd2ffda999934c5ae"),
        "{}",
        request.url
    );
    let variables = graphql_variables(request);
    assert_eq!(variables["id"], "U3RheUxpc3Rpbmc6Mzg4MTc5Njk=");
    assert_eq!(variables["pdpReviewsRequest"]["offset"], "0");
    assert_eq!(variables["pdpReviewsRequest"]["limit"], 24);
}

#[tokio::test]
async fn graphql_reviews_second_page_sends_the_requested_offset() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    mount_reviews_capture(&server).await;

    let page = client.get_reviews("38817969", Some("24")).await.unwrap();
    assert_eq!(page.next_cursor.as_deref(), Some("48"));

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    let variables = graphql_variables(recorded_reviews_request(&requests));
    assert_eq!(variables["pdpReviewsRequest"]["offset"], "24");
}

#[tokio::test]
async fn graphql_reviews_reject_a_non_numeric_cursor_without_http() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpReviewsQuery/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REVIEWS_CAPTURE))
        .expect(0)
        .mount(&server)
        .await;

    let err = client
        .get_reviews("38817969", Some("abc"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AirbnbError::InvalidParams { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn graphql_search_cache_key_covers_every_filter() {
    let server = MockServer::start().await;
    let cache = Arc::new(MemoryCache::new(100));
    let client = build_client_with_cache(&server, cache).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .expect(6) // six distinct parameter sets, then one cache hit
        .mount(&server)
        .await;

    let base = base_params();
    let mut dated = base_params();
    let (checkin, checkout) = future_stay();
    dated.checkin = Some(checkin);
    dated.checkout = Some(checkout);
    let mut guests = base_params();
    guests.adults = Some(3);
    let mut priced = base_params();
    priced.max_price = Some(120);
    let mut typed = base_params();
    typed.property_type = Some("Private room".into());
    let mut paged = base_params();
    paged.cursor = Some("CURSOR_2".into());

    for params in [&base, &dated, &guests, &priced, &typed, &paged] {
        client.search_listings(params).await.unwrap();
    }
    // Same parameters again: served from the cache, no seventh request.
    client.search_listings(&dated).await.unwrap();
}

#[tokio::test]
async fn graphql_search_posts_cursor_filters_and_current_hash() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response_json()))
        .mount(&server)
        .await;

    let mut params = base_params();
    params.cursor = Some("CURSOR_2".into());
    params.property_type = Some("Hotel room".into());
    let (checkin, checkout) = future_stay();
    params.checkin = Some(checkin.clone());
    params.checkout = Some(checkout);
    client.search_listings(&params).await.unwrap();

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    let post = requests
        .iter()
        .find(|r| r.method.as_str() == "POST")
        .expect("a StaysSearch POST was sent");
    assert!(
        post.url
            .path()
            .ends_with("/0afc7d440ee66286e44038530dc8d2af77d795e434e5cc5c8a8034c93cb377cf"),
        "{}",
        post.url
    );
    let body: serde_json::Value = post.body_json().expect("JSON body");
    assert_eq!(body["operationName"], "StaysSearch");
    assert_eq!(
        body["extensions"]["persistedQuery"]["sha256Hash"],
        "0afc7d440ee66286e44038530dc8d2af77d795e434e5cc5c8a8034c93cb377cf"
    );
    let request = &body["variables"]["staysSearchRequest"];
    assert_eq!(request["cursor"], "CURSOR_2");
    let raw = request["rawParams"].as_array().expect("rawParams");
    let first_value = |name: &str| {
        raw.iter()
            .find(|p| p["filterName"] == name)
            .map(|p| p["filterValues"][0].clone())
    };
    assert_eq!(first_value("query"), Some(serde_json::json!("Paris")));
    assert_eq!(
        first_value("kgAndTags"),
        Some(serde_json::json!("Tag:9613"))
    );
    assert_eq!(first_value("roomTypes"), None);
    assert_eq!(first_value("checkin"), Some(serde_json::json!(checkin)));
    assert_eq!(first_value("placeId"), None);
}

#[tokio::test]
async fn graphql_search_parses_2026_09_web_capture() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    let capture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/airbnb/2026-09/graphql/StaysSearch.response.json"
    ))
    .unwrap();

    Mock::given(method("POST"))
        .and(path_regex("/api/v3/StaysSearch/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&capture))
        .mount(&server)
        .await;

    let mut params = base_params();
    params.location = "Lyon, France".into();
    let result = client.search_listings(&params).await.unwrap();
    assert_eq!(result.listings.len(), 18);
    assert!(
        result
            .listings
            .iter()
            .all(|l| !l.id.is_empty() && l.id.chars().all(|c| c.is_ascii_digit()))
    );
    let page_cursors = capture["data"]["presentation"]["staysSearch"]["results"]["paginationInfo"]
        ["pageCursors"]
        .as_array()
        .expect("page cursors");
    assert_eq!(result.next_cursor.as_deref(), page_cursors[1].as_str());
}

#[tokio::test]
async fn graphql_calendar_request_matches_web_client_and_keeps_null_prices() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/PdpAvailabilityCalendar/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(include_str!(
            "fixtures/airbnb/2026-09/graphql/PdpAvailabilityCalendar.response.json"
        )))
        .mount(&server)
        .await;

    let calendar = client.get_price_calendar("38817969", 3).await.unwrap();
    assert_eq!(calendar.days.len(), 365);
    assert!(calendar.days.iter().all(|d| d.price.is_none()));
    assert!(calendar.average_price.is_none());

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    let request = requests
        .iter()
        .find(|r| r.url.path().starts_with("/api/v3/PdpAvailabilityCalendar/"))
        .expect("a calendar request was sent");
    assert!(
        request
            .url
            .path()
            .contains("be60714ead0a30db42ce6471ddad6a8f3855df0ed400b79282dd0bb8cecdf201"),
        "{}",
        request.url
    );
    let variables = graphql_variables(request);
    assert_eq!(variables["request"]["listingId"], "38817969");
    assert_eq!(variables["request"]["count"], 3);
    assert_eq!(
        variables["request"]["returnPropertyLevelCalendarIfApplicable"],
        false
    );
}

#[tokio::test]
async fn graphql_refreshes_a_rejected_api_key_only_once() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<script>{"api_config":{"key":"key-one"}}</script>"#),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<script>{"api_config":{"key":"key-two"}}</script>"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path_regex("/api/v3/StaysPdpSections/.*"))
        .and(header("X-Airbnb-Api-Key", "key-one"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path_regex("/api/v3/StaysPdpSections/.*"))
        .and(header("X-Airbnb-Api-Key", "key-two"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;

    let client = AirbnbGraphQLClient::new(
        &fast_graphql_config(&server.uri()),
        test_cache_config(),
        Arc::new(MemoryCache::new(100)),
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap();

    let err = client
        .get_listing_detail("501")
        .await
        .expect_err("both keys are rejected");
    assert!(
        matches!(err, AirbnbError::UpstreamStatus { status: 403, .. }),
        "{err}"
    );
    // wiremock verifies on drop: two homepage fetches, one request per key.
}

#[tokio::test]
async fn graphql_retries_server_errors_up_to_max_retries() {
    let server = MockServer::start().await;
    mount_api_key_mock(&server).await;
    Mock::given(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&server)
        .await;

    let mut config = fast_graphql_config(&server.uri());
    config.max_retries = 1;
    let client = AirbnbGraphQLClient::new(
        &config,
        test_cache_config(),
        Arc::new(MemoryCache::new(100)),
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap();

    let err = client
        .get_listing_detail("501")
        .await
        .expect_err("503 twice");
    assert!(
        matches!(err, AirbnbError::UpstreamStatus { status: 503, .. }),
        "{err}"
    );
}

#[tokio::test]
async fn graphql_422_on_a_persisted_query_is_upstream_schema() {
    let server = MockServer::start().await;
    mount_api_key_mock(&server).await;
    Mock::given(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(422))
        .expect(1)
        .mount(&server)
        .await;

    let client = AirbnbGraphQLClient::new(
        &fast_graphql_config(&server.uri()),
        test_cache_config(),
        Arc::new(MemoryCache::new(100)),
        test_api_key_manager(&server.uri()),
        test_limiter(),
    )
    .unwrap();

    let err = client
        .get_listing_detail("501")
        .await
        .expect_err("stale hash");
    assert!(matches!(err, AirbnbError::UpstreamSchema { .. }), "{err}");
    assert!(err.to_string().contains("graphql_hashes"), "{err}");
}
