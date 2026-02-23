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
    );

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
    );

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
    );

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
    );

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
    let scraper = AirbnbScraper::new(config, test_cache_config(), cache, api_key_mgr);

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
    let scraper = AirbnbScraper::new(config, test_cache_config(), cache, api_key_mgr);

    let result = scraper.get_listing_detail("501").await;
    assert!(result.is_err());
}
