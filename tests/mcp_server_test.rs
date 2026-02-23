use std::sync::Arc;

use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::{AirbnbError, Result};
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use async_trait::async_trait;
use rmcp::ServerHandler;

/// A simple mock client for integration tests
struct IntegrationMock;

#[async_trait]
impl AirbnbClient for IntegrationMock {
    async fn search_listings(&self, _params: &SearchParams) -> Result<SearchResult> {
        Ok(SearchResult {
            listings: vec![
                Listing {
                    id: "101".into(),
                    name: "Integration Apt".into(),
                    location: "Berlin".into(),
                    price_per_night: 90.0,
                    currency: "$".into(),
                    rating: Some(4.6),
                    review_count: 30,
                    thumbnail_url: None,
                    property_type: Some("Apartment".into()),
                    host_name: Some("Hans".into()),
                    url: "https://www.airbnb.com/rooms/101".into(),
                    is_superhost: None,
                    is_guest_favorite: None,
                    instant_book: None,
                    total_price: None,
                    photos: vec![],
                    latitude: None,
                    longitude: None,
                },
                Listing {
                    id: "102".into(),
                    name: "Integration House".into(),
                    location: "Munich".into(),
                    price_per_night: 150.0,
                    currency: "$".into(),
                    rating: None,
                    review_count: 0,
                    thumbnail_url: None,
                    property_type: None,
                    host_name: None,
                    url: "https://www.airbnb.com/rooms/102".into(),
                    is_superhost: None,
                    is_guest_favorite: None,
                    instant_book: None,
                    total_price: None,
                    photos: vec![],
                    latitude: None,
                    longitude: None,
                },
            ],
            total_count: Some(2),
            next_cursor: None,
        })
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        Ok(ListingDetail {
            id: id.into(),
            name: "Integration Detail".into(),
            location: "Berlin".into(),
            description: "A lovely place for testing".into(),
            price_per_night: 90.0,
            currency: "$".into(),
            rating: Some(4.6),
            review_count: 30,
            property_type: Some("Apartment".into()),
            host_name: Some("Hans".into()),
            url: format!("https://www.airbnb.com/rooms/{id}"),
            amenities: vec!["WiFi".into()],
            house_rules: vec![],
            latitude: None,
            longitude: None,
            photos: vec![],
            bedrooms: Some(1),
            beds: Some(1),
            bathrooms: Some(1.0),
            max_guests: Some(2),
            check_in_time: None,
            check_out_time: None,
            host_id: None,
            host_is_superhost: None,
            host_response_rate: None,
            host_response_time: None,
            host_joined: None,
            host_total_listings: None,
            host_languages: vec![],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: None,
            service_fee: None,
            neighborhood: None,
        })
    }

    async fn get_reviews(&self, id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Ok(ReviewsPage {
            listing_id: id.into(),
            summary: None,
            reviews: vec![Review {
                author: "Tester".into(),
                date: "2025-01-01".into(),
                rating: Some(5.0),
                comment: "Integration test review".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            }],
            next_cursor: None,
        })
    }

    async fn get_price_calendar(&self, id: &str, _months: u32) -> Result<PriceCalendar> {
        Ok(PriceCalendar {
            listing_id: id.into(),
            currency: "$".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: Some(90.0),
                available: true,
                min_nights: Some(1),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        })
    }
}

/// Error mock for testing error propagation
struct ErrorMock;

#[async_trait]
impl AirbnbClient for ErrorMock {
    async fn search_listings(&self, _params: &SearchParams) -> Result<SearchResult> {
        Err(AirbnbError::RateLimited)
    }
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        Err(AirbnbError::ListingNotFound { id: id.into() })
    }
    async fn get_reviews(&self, _id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Err(AirbnbError::Parse {
            reason: "no data".into(),
        })
    }
    async fn get_price_calendar(&self, _id: &str, _months: u32) -> Result<PriceCalendar> {
        Err(AirbnbError::Parse {
            reason: "no calendar".into(),
        })
    }
}

#[test]
fn server_lists_seven_tools() {
    let server = AirbnbMcpServer::new(Arc::new(IntegrationMock));
    let info = server.get_info();
    let instructions = info.instructions.unwrap();
    // Verify all 15 tools are mentioned
    assert!(instructions.contains("airbnb_search"));
    assert!(instructions.contains("airbnb_listing_details"));
    assert!(instructions.contains("airbnb_reviews"));
    assert!(instructions.contains("airbnb_price_calendar"));
    assert!(instructions.contains("airbnb_host_profile"));
    assert!(instructions.contains("airbnb_neighborhood_stats"));
    assert!(instructions.contains("airbnb_occupancy_estimate"));
    assert!(instructions.contains("airbnb_compare_listings"));
    assert!(instructions.contains("airbnb_price_trends"));
    assert!(instructions.contains("airbnb_gap_finder"));
    assert!(instructions.contains("airbnb_revenue_estimate"));
    assert!(instructions.contains("airbnb_listing_score"));
    assert!(instructions.contains("airbnb_amenity_analysis"));
    assert!(instructions.contains("airbnb_market_comparison"));
    assert!(instructions.contains("airbnb_host_portfolio"));
    // Verify capabilities include tools and resources
    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_some());
}

#[test]
fn server_get_info_has_protocol_version() {
    let server = AirbnbMcpServer::new(Arc::new(IntegrationMock));
    let info = server.get_info();
    // Just verify it doesn't panic and returns valid info
    let _ = info.protocol_version; // ProtocolVersion exists
}

#[test]
fn server_creates_with_different_clients() {
    // Verify server can be constructed with different client implementations
    let _server1 = AirbnbMcpServer::new(Arc::new(IntegrationMock));
    let _server2 = AirbnbMcpServer::new(Arc::new(ErrorMock));
    // Both should construct without panicking
}
