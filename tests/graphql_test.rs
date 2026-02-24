use std::sync::Arc;

use mcp_airbnb::adapters::cache::memory_cache::MemoryCache;
use mcp_airbnb::adapters::graphql::client::AirbnbGraphQLClient;
use mcp_airbnb::adapters::shared::ApiKeyManager;
use mcp_airbnb::config::types::{CacheConfig, ScraperConfig};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use wiremock::matchers::{method, path, path_regex};
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
    Arc::new(ApiKeyManager::new(http, base_url.to_string(), 86400))
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
                        "year": 2026,
                        "days": [
                            { "calendarDate": "2026-03-01", "available": true, "price": { "amount": 120.0 }, "minNights": 2, "maxNights": 30 },
                            { "calendarDate": "2026-03-02", "available": true, "price": { "amount": 130.0 }, "minNights": 2, "maxNights": 30 },
                            { "calendarDate": "2026-03-03", "available": false, "price": { "amount": 120.0 }, "minNights": 2, "maxNights": 30 }
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
async fn graphql_get_500_returns_parse_error() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;

    Mock::given(method("GET"))
        .and(path_regex("/api/v3/StaysPdpSections/.*"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let result = client.get_listing_detail("501").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("HTTP"));
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

    let result = client.search_listings(&base_params()).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("could not find")
    );
}

#[tokio::test]
async fn graphql_api_key_extraction_failure() {
    let server = MockServer::start().await;

    // Mount homepage that has no API key
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<html>No config here</html>"),
        )
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
    )
    .unwrap();

    let result = client.get_listing_detail("501").await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("API key")
    );
}
