use std::sync::Arc;

use mcp_airbnb::adapters::cache::memory_cache::MemoryCache;
use mcp_airbnb::adapters::scraper::client::AirbnbScraper;
use mcp_airbnb::adapters::shared::ApiKeyManager;
use mcp_airbnb::config::types::{CacheConfig, ScraperConfig};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fast_scraper_config(base_url: &str) -> ScraperConfig {
    ScraperConfig {
        base_url: base_url.to_string(),
        rate_limit_per_second: 100.0, // fast for tests
        request_timeout_secs: 5,
        max_retries: 1,
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

fn search_html() -> String {
    r#"<html><head><script id="__NEXT_DATA__" type="application/json">
    {"props":{"pageProps":{"searchResults":[
        {"listing":{"id":"501","name":"Mock Listing","city":"MockCity","avgRating":4.5,"reviewsCount":20},"pricingQuote":{"price":{"amount":120.0}}}
    ]}}}
    </script></head><body></body></html>"#
        .to_string()
}

fn detail_html() -> String {
    r#"<html><head><script id="__NEXT_DATA__" type="application/json">
    {"props":{"pageProps":{"listing":{
        "name":"Mock Detail",
        "description":"A mock listing for testing",
        "city":"MockCity",
        "price":120.0,
        "avgRating":4.5,
        "reviewsCount":20,
        "amenities":[{"name":"WiFi"}]
    }}}}
    </script></head><body></body></html>"#
        .to_string()
}

fn reviews_html() -> String {
    r#"<html><head><script id="__NEXT_DATA__" type="application/json">
    {"props":{"pageProps":{
        "reviews":[
            {"reviewer":{"firstName":"Alice"},"comments":"Wonderful place, very clean!","createdAt":"2025-03-15","rating":5.0},
            {"reviewer":{"firstName":"Bob"},"comments":"Great location and host.","createdAt":"2025-02-20","rating":4.0,
             "response":{"comments":"Thank you Bob!"}}
        ],
        "listing":{
            "avgRating":4.8,
            "reviewsCount":42,
            "cleanlinessRating":4.9,
            "accuracyRating":4.7,
            "communicationRating":5.0,
            "locationRating":4.6,
            "checkinRating":4.8,
            "valueRating":4.5
        }
    }}}
    </script></head><body></body></html>"#
        .to_string()
}

fn calendar_html() -> String {
    r#"<html><head><script id="__NEXT_DATA__" type="application/json">
    {"props":{"pageProps":{"calendarData":{
        "calendarMonths":[
            {"month":3,"year":2026,"days":[
                {"date":"2026-03-01","available":true,"price":{"amount":150.0},"minNights":2},
                {"date":"2026-03-02","available":true,"price":{"amount":160.0},"minNights":2},
                {"date":"2026-03-03","available":false,"price":{"amount":170.0},"minNights":2}
            ]},
            {"month":4,"year":2026,"days":[
                {"date":"2026-04-01","available":true,"price":{"amount":140.0},"minNights":1}
            ]}
        ],
        "currency":"USD"
    }}}}
    </script></head><body></body></html>"#
        .to_string()
}

fn host_profile_html() -> String {
    // The host profile parser only supports data-deferred-state / niobeClientData format,
    // not __NEXT_DATA__. The structure must have a MEET_YOUR_HOST section with cardData.
    r#"<html><head><script data-deferred-state="true" type="application/json">
    {"niobeClientData":[["StaysPdpSections:test",{
        "data":{"presentation":{"stayProductDetailPage":{
            "sections":{
                "metadata":{},
                "sections":[
                    {"sectionComponentType":"MEET_YOUR_HOST","section":{
                        "cardData":{
                            "name":"Maria",
                            "userId":"12345",
                            "isSuperhost":true,
                            "responseRate":"99%",
                            "responseTime":"within an hour",
                            "languages":["English","Spanish","French"],
                            "listingsCount":5,
                            "about":"Passionate host who loves welcoming guests.",
                            "isIdentityVerified":true
                        }
                    }}
                ]
            }
        }}}
    }]]}
    </script></head><body></body></html>"#
        .to_string()
}

#[tokio::test]
async fn scraper_search_parses_html_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/s/.*/homes"))
        .respond_with(ResponseTemplate::new(200).set_body_string(search_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let params = SearchParams {
        location: "MockCity".into(),
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

    let result = scraper.search_listings(&params).await.unwrap();
    assert_eq!(result.listings.len(), 1);
    assert_eq!(result.listings[0].id, "501");
    assert_eq!(result.listings[0].name, "Mock Listing");
}

#[tokio::test]
async fn scraper_detail_parses_html_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(detail_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let detail = scraper.get_listing_detail("501").await.unwrap();
    assert_eq!(detail.name, "Mock Detail");
    assert_eq!(detail.amenities, vec!["WiFi"]);
}

#[tokio::test]
async fn scraper_caches_results() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(detail_html()))
        .expect(1) // Should only receive 1 request; second is cached
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    // First call — hits the mock server
    let detail1 = scraper.get_listing_detail("501").await.unwrap();
    // Second call — should use cache
    let detail2 = scraper.get_listing_detail("501").await.unwrap();

    assert_eq!(detail1.name, detail2.name);
    // wiremock will verify expect(1) on drop
}

#[tokio::test]
async fn scraper_retries_on_server_error() {
    let mock_server = MockServer::start().await;

    // First request returns 500, second returns 200
    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(detail_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let detail = scraper.get_listing_detail("501").await.unwrap();
    assert_eq!(detail.name, "Mock Detail");
}

#[tokio::test]
async fn scraper_404_returns_listing_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(404).set_body_string("<html>Not Found</html>"))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let mut config = fast_scraper_config(&mock_server.uri());
    config.max_retries = 0; // no retries for 404
    let scraper = AirbnbScraper::new(config, test_cache_config(), cache, api_key_mgr).unwrap();

    let result = scraper.get_listing_detail("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn scraper_429_returns_rate_limited() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let mut config = fast_scraper_config(&mock_server.uri());
    config.max_retries = 0;
    let scraper = AirbnbScraper::new(config, test_cache_config(), cache, api_key_mgr).unwrap();

    let result = scraper.get_listing_detail("501").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn scraper_reviews_parses_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(reviews_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let page = scraper.get_reviews("501", None).await.unwrap();

    // Verify reviews were parsed
    assert_eq!(page.listing_id, "501");
    assert_eq!(page.reviews.len(), 2);

    // Verify first review fields
    assert_eq!(page.reviews[0].author, "Alice");
    assert_eq!(page.reviews[0].comment, "Wonderful place, very clean!");
    assert_eq!(page.reviews[0].date, "2025-03-15");
    assert_eq!(page.reviews[0].rating, Some(5.0));
    assert!(page.reviews[0].response.is_none());

    // Verify second review with host response
    assert_eq!(page.reviews[1].author, "Bob");
    assert_eq!(page.reviews[1].comment, "Great location and host.");
    assert_eq!(page.reviews[1].rating, Some(4.0));
    assert_eq!(page.reviews[1].response, Some("Thank you Bob!".to_string()));

    // Verify reviews summary
    let summary = page.summary.expect("summary should be present");
    assert!((summary.overall_rating - 4.8).abs() < f64::EPSILON);
    assert_eq!(summary.total_reviews, 42);
    assert_eq!(summary.cleanliness, Some(4.9));
    assert_eq!(summary.accuracy, Some(4.7));
    assert_eq!(summary.communication, Some(5.0));
    assert_eq!(summary.location, Some(4.6));
    assert_eq!(summary.check_in, Some(4.8));
    assert_eq!(summary.value, Some(4.5));
}

#[tokio::test]
async fn scraper_calendar_parses_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(calendar_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let calendar = scraper.get_price_calendar("501", 3).await.unwrap();

    // Verify calendar metadata
    assert_eq!(calendar.listing_id, "501");
    assert_eq!(calendar.currency, "USD");

    // Verify all days across both months were parsed
    assert_eq!(calendar.days.len(), 4);

    // Verify first month days
    assert_eq!(calendar.days[0].date, "2026-03-01");
    assert!(calendar.days[0].available);
    assert_eq!(calendar.days[0].price, Some(150.0));
    assert_eq!(calendar.days[0].min_nights, Some(2));

    assert_eq!(calendar.days[1].date, "2026-03-02");
    assert!(calendar.days[1].available);
    assert_eq!(calendar.days[1].price, Some(160.0));

    assert_eq!(calendar.days[2].date, "2026-03-03");
    assert!(!calendar.days[2].available);
    assert_eq!(calendar.days[2].price, Some(170.0));

    // Verify second month day
    assert_eq!(calendar.days[3].date, "2026-04-01");
    assert!(calendar.days[3].available);
    assert_eq!(calendar.days[3].price, Some(140.0));
    assert_eq!(calendar.days[3].min_nights, Some(1));

    // Verify computed stats (compute_stats is called by the parser)
    // Available days with prices: 150, 160, 140 => avg = 150.0
    assert!(calendar.average_price.is_some());
    assert!((calendar.average_price.unwrap() - 150.0).abs() < 0.01);
    assert_eq!(calendar.min_price, Some(140.0));
    assert_eq!(calendar.max_price, Some(160.0));

    // Occupancy: 1 unavailable out of 4 total => 25%
    assert!(calendar.occupancy_rate.is_some());
    assert!((calendar.occupancy_rate.unwrap() - 25.0).abs() < 0.01);
}

#[tokio::test]
async fn scraper_host_profile_parses_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/rooms/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(host_profile_html()))
        .mount(&mock_server)
        .await;

    let cache = Arc::new(MemoryCache::new(100));
    let api_key_mgr = test_api_key_manager(&mock_server.uri());
    let scraper = AirbnbScraper::new(
        fast_scraper_config(&mock_server.uri()),
        test_cache_config(),
        cache,
        api_key_mgr,
    )
    .unwrap();

    let profile = scraper.get_host_profile("501").await.unwrap();

    // Verify host identity
    assert_eq!(profile.name, "Maria");
    assert_eq!(profile.host_id, Some("12345".to_string()));

    // Verify superhost status
    assert_eq!(profile.is_superhost, Some(true));

    // Verify response metrics
    assert_eq!(profile.response_rate, Some("99%".to_string()));
    assert_eq!(profile.response_time, Some("within an hour".to_string()));

    // Verify languages
    assert_eq!(profile.languages, vec!["English", "Spanish", "French"]);

    // Verify listing count
    assert_eq!(profile.total_listings, Some(5));

    // Verify description
    assert_eq!(
        profile.description,
        Some("Passionate host who loves welcoming guests.".to_string())
    );

    // Verify identity verification
    assert_eq!(profile.identity_verified, Some(true));
}
