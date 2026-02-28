use std::sync::Arc;

use mcp_airbnb::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
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
                    host_id: None,
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
                    host_id: None,
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
                unavailability_reason: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        })
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Ok(HostProfile {
            host_id: Some("host-1".into()),
            name: "Hans".into(),
            is_superhost: Some(true),
            response_rate: Some("95%".into()),
            response_time: Some("within an hour".into()),
            member_since: Some("2020".into()),
            languages: vec!["English".into(), "German".into()],
            total_listings: Some(2),
            description: None,
            profile_picture_url: None,
            identity_verified: Some(true),
        })
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        Ok(NeighborhoodStats {
            location: params.location.clone(),
            total_listings: 2,
            average_price: Some(120.0),
            median_price: Some(110.0),
            price_range: Some((80.0, 160.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(50.0),
        })
    }

    async fn get_occupancy_estimate(&self, id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Ok(OccupancyEstimate {
            listing_id: id.into(),
            period_start: "2025-06-01".into(),
            period_end: "2025-08-31".into(),
            total_days: 92,
            occupied_days: 60,
            available_days: 32,
            occupancy_rate: 65.2,
            average_available_price: Some(90.0),
            weekend_avg_price: Some(110.0),
            weekday_avg_price: Some(80.0),
            monthly_breakdown: vec![],
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

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Err(AirbnbError::Parse {
            reason: "no host".into(),
        })
    }

    async fn get_neighborhood_stats(&self, _params: &SearchParams) -> Result<NeighborhoodStats> {
        Err(AirbnbError::Parse {
            reason: "no stats".into(),
        })
    }

    async fn get_occupancy_estimate(&self, _id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Err(AirbnbError::Parse {
            reason: "no occupancy".into(),
        })
    }
}

#[test]
fn server_lists_seven_tools() {
    let server = AirbnbMcpServer::new(Arc::new(IntegrationMock));
    let info = server.get_info();
    let instructions = info.instructions.unwrap();
    // Verify all 18 tools are mentioned
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
    assert!(instructions.contains("airbnb_review_sentiment"));
    assert!(instructions.contains("airbnb_competitive_positioning"));
    assert!(instructions.contains("airbnb_optimal_pricing"));
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
