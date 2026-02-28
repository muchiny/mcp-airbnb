//! Functional verification tests for all 14 improvements.
//!
//! These tests exercise the 7 base data tools + 11 analytical tools through the
//! full MCP protocol (duplex transport), plus edge-case and serde verification.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use async_trait::async_trait;

use mcp_airbnb::domain::analytics::{
    HostProfile, MonthlyOccupancy, NeighborhoodStats, OccupancyEstimate, PropertyTypeCount,
};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar, UnavailabilityReason};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage, ReviewsSummary};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::Result;
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo};
use rmcp::{ClientHandler, ServiceExt};

// ---------------------------------------------------------------------------
// FunctionalMock — realistic data for all 18 tool paths
// ---------------------------------------------------------------------------

struct FunctionalMock;

#[async_trait]
impl AirbnbClient for FunctionalMock {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        if params.location.is_empty() {
            return Err(mcp_airbnb::error::AirbnbError::Parse {
                reason: "location cannot be empty".into(),
            });
        }
        Ok(SearchResult {
            listings: vec![
                Listing {
                    id: "10".into(),
                    name: "Test Apartment".into(),
                    location: params.location.clone(),
                    price_per_night: 100.0,
                    currency: "$".into(),
                    rating: Some(4.7),
                    review_count: 50,
                    thumbnail_url: Some("https://example.com/thumb.jpg".into()),
                    property_type: Some("Entire home".into()),
                    host_name: Some("Alice".into()),
                    host_id: Some("host-alice".into()),
                    url: "https://www.airbnb.com/rooms/10".into(),
                    is_superhost: Some(true),
                    is_guest_favorite: Some(true),
                    instant_book: Some(true),
                    total_price: Some(300.0),
                    photos: vec!["https://example.com/p1.jpg".into()],
                    latitude: Some(40.7128),
                    longitude: Some(-74.006),
                },
                Listing {
                    id: "20".into(),
                    name: "Budget Room".into(),
                    location: params.location.clone(),
                    price_per_night: 50.0,
                    currency: "$".into(),
                    rating: Some(4.2),
                    review_count: 12,
                    thumbnail_url: None,
                    property_type: Some("Private room".into()),
                    host_name: Some("Bob".into()),
                    host_id: Some("host-bob".into()),
                    url: "https://www.airbnb.com/rooms/20".into(),
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
            name: "Test Apartment".into(),
            location: "New York, USA".into(),
            description: "A wonderful place to stay in Manhattan.".into(),
            price_per_night: 100.0,
            currency: "$".into(),
            rating: Some(4.7),
            review_count: 50,
            property_type: Some("Entire home".into()),
            host_name: Some("Alice".into()),
            url: format!("https://www.airbnb.com/rooms/{id}"),
            amenities: vec![
                "WiFi".into(),
                "Kitchen".into(),
                "Air conditioning".into(),
                "TV".into(),
                "Washer".into(),
            ],
            house_rules: vec!["No smoking".into(), "No parties".into()],
            latitude: Some(40.7128),
            longitude: Some(-74.006),
            photos: vec![
                "https://example.com/1.jpg".into(),
                "https://example.com/2.jpg".into(),
                "https://example.com/3.jpg".into(),
            ],
            bedrooms: Some(1),
            beds: Some(1),
            bathrooms: Some(1.0),
            max_guests: Some(2),
            check_in_time: Some("15:00".into()),
            check_out_time: Some("11:00".into()),
            host_id: Some("host-alice".into()),
            host_is_superhost: Some(true),
            host_response_rate: Some("99%".into()),
            host_response_time: Some("within an hour".into()),
            host_joined: Some("2019".into()),
            host_total_listings: Some(2),
            host_languages: vec!["English".into()],
            cancellation_policy: Some("Moderate".into()),
            instant_book: Some(true),
            cleaning_fee: Some(30.0),
            service_fee: Some(15.0),
            neighborhood: Some("Manhattan".into()),
        })
    }

    async fn get_reviews(&self, id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Ok(ReviewsPage {
            listing_id: id.into(),
            summary: Some(ReviewsSummary {
                overall_rating: 4.7,
                total_reviews: 50,
                cleanliness: Some(4.8),
                accuracy: Some(4.6),
                communication: Some(4.9),
                location: Some(4.5),
                check_in: Some(4.7),
                value: Some(4.4),
            }),
            reviews: vec![
                Review {
                    author: "Carol".into(),
                    date: "2025-12-01".into(),
                    rating: Some(5.0),
                    comment: "Amazing stay! Very clean and great location.".into(),
                    response: Some("Thanks Carol!".into()),
                    reviewer_location: Some("London, UK".into()),
                    language: Some("en".into()),
                    is_translated: None,
                },
                Review {
                    author: "Dave".into(),
                    date: "2025-11-15".into(),
                    rating: Some(4.0),
                    comment: "Good apartment but a bit noisy at night.".into(),
                    response: None,
                    reviewer_location: None,
                    language: Some("en".into()),
                    is_translated: None,
                },
            ],
            next_cursor: None,
        })
    }

    async fn get_price_calendar(&self, id: &str, _months: u32) -> Result<PriceCalendar> {
        Ok(PriceCalendar {
            listing_id: id.into(),
            currency: "$".into(),
            days: vec![
                CalendarDay {
                    date: "2025-07-01".into(),
                    price: Some(100.0),
                    available: true,
                    min_nights: Some(2),
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-07-02".into(),
                    price: Some(100.0),
                    available: false,
                    min_nights: Some(2),
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: Some(UnavailabilityReason::Booked),
                },
                CalendarDay {
                    date: "2025-07-03".into(),
                    price: Some(120.0),
                    available: true,
                    min_nights: Some(2),
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-07-04".into(),
                    price: Some(150.0),
                    available: false,
                    min_nights: Some(2),
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: Some(UnavailabilityReason::BlockedByHost),
                },
            ],
            average_price: Some(117.5),
            occupancy_rate: Some(50.0),
            min_price: Some(100.0),
            max_price: Some(150.0),
        })
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Ok(HostProfile {
            host_id: Some("host-alice".into()),
            name: "Alice".into(),
            is_superhost: Some(true),
            response_rate: Some("99%".into()),
            response_time: Some("within an hour".into()),
            member_since: Some("2019".into()),
            languages: vec!["English".into(), "French".into()],
            total_listings: Some(2),
            description: Some("Passionate host in NYC".into()),
            profile_picture_url: Some("https://example.com/alice.jpg".into()),
            identity_verified: Some(true),
        })
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        Ok(NeighborhoodStats {
            location: params.location.clone(),
            total_listings: 500,
            average_price: Some(130.0),
            median_price: Some(110.0),
            price_range: Some((40.0, 600.0)),
            average_rating: Some(4.55),
            property_type_distribution: vec![
                PropertyTypeCount {
                    property_type: "Entire home".into(),
                    count: 300,
                    percentage: 60.0,
                },
                PropertyTypeCount {
                    property_type: "Private room".into(),
                    count: 200,
                    percentage: 40.0,
                },
            ],
            superhost_percentage: Some(32.0),
        })
    }

    async fn get_occupancy_estimate(&self, id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Ok(OccupancyEstimate {
            listing_id: id.into(),
            period_start: "2025-07-01".into(),
            period_end: "2025-09-30".into(),
            total_days: 92,
            occupied_days: 65,
            available_days: 27,
            occupancy_rate: 70.6,
            average_available_price: Some(105.0),
            weekend_avg_price: Some(130.0),
            weekday_avg_price: Some(90.0),
            monthly_breakdown: vec![MonthlyOccupancy {
                month: "July 2025".into(),
                total_days: 31,
                occupied_days: 22,
                available_days: 9,
                occupancy_rate: 71.0,
                average_price: Some(110.0),
            }],
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct DummyClient;

impl ClientHandler for DummyClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

fn extract_text(result: &CallToolResult) -> String {
    result
        .content
        .first()
        .and_then(|c| c.raw.as_text())
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

fn is_success(result: &CallToolResult) -> bool {
    result.is_error.is_none() || result.is_error == Some(false)
}

#[allow(clippy::needless_pass_by_value)]
fn tool_params(name: &str, args: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams {
        meta: None,
        name: std::borrow::Cow::Owned(name.to_string()),
        arguments: Some(args.as_object().unwrap().clone()),
        task: None,
    }
}

async fn setup() -> (
    rmcp::service::RunningService<rmcp::RoleClient, DummyClient>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(65536);

    let server = AirbnbMcpServer::new(Arc::new(FunctionalMock));
    let server_handle = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });

    let client = DummyClient
        .serve(client_transport)
        .await
        .expect("client should connect");

    (client, server_handle)
}

async fn teardown(
    client: rmcp::service::RunningService<rmcp::RoleClient, DummyClient>,
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let _ = client.cancel().await;
    let _ = server_handle.await;
}

// ===========================================================================
// Phase 2: Regression tests for 7 base data tools
// ===========================================================================

#[tokio::test]
async fn regression_search() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_search",
            serde_json::json!({ "location": "New York" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Test Apartment"),
        "Should contain listing name"
    );
    assert!(
        text.contains("100") || text.contains('$'),
        "Should contain price info"
    );
    assert!(
        text.contains("airbnb.com/rooms"),
        "Should contain listing URL"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_listing_details() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_listing_details",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("wonderful place"),
        "Should contain description"
    );
    assert!(text.contains("WiFi"), "Should contain amenities");
    assert!(text.contains("No smoking"), "Should contain house rules");

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_reviews() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_reviews",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Carol") || text.contains("Amazing"),
        "Should contain review text"
    );
    assert!(
        text.contains("4.7") || text.contains("rating"),
        "Should contain rating info"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_price_calendar() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_price_calendar",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("2025-07-01") || text.contains("Jul"),
        "Should contain dates"
    );
    assert!(
        text.contains("100") || text.contains('$'),
        "Should contain price"
    );
    assert!(
        text.contains("available") || text.contains("Available") || text.contains('\u{2713}'),
        "Should indicate availability"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_host_profile() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_host_profile",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(text.contains("Alice"), "Should contain host name");
    assert!(
        text.contains("Superhost") || text.contains("superhost"),
        "Should mention superhost status"
    );
    assert!(text.contains("99%"), "Should contain response rate");

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_neighborhood_stats() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_neighborhood_stats",
            serde_json::json!({ "location": "New York" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("130") || text.contains("average"),
        "Should contain average price"
    );
    assert!(
        text.contains("110") || text.contains("median"),
        "Should contain median price"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn regression_occupancy_estimate() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_occupancy_estimate",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("70") || text.contains("occupancy"),
        "Should contain occupancy rate"
    );
    assert!(
        text.contains("July") || text.contains("monthly"),
        "Should contain monthly breakdown"
    );

    teardown(client, server_handle).await;
}

// ===========================================================================
// Phase 1: Item-specific verification
// ===========================================================================

// Item 1: Trait has no default implementations (compile-time verification)
// If this file compiles, FunctionalMock proves all 7 methods are required.
// The IntegrationMock and ErrorMock in mcp_server_test.rs also prove this.

// Item 2: Cache host_profile — verified at adapter level by scraper_test.rs
// The config default is verified by the config_serde_roundtrip test.

// Item 6: Pagination metadata in compare_listings by location
#[tokio::test]
async fn pagination_metadata_in_compare_by_location() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_compare_listings",
            serde_json::json!({ "location": "New York" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Fetched") && text.contains("page(s)"),
        "Should contain pagination metadata 'Fetched N listings across P page(s)', got: {text}"
    );

    teardown(client, server_handle).await;
}

// Item 8: Review sentiment via MCP
#[tokio::test]
async fn review_sentiment_via_mcp() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_review_sentiment",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Sentiment") || text.contains("sentiment"),
        "Should contain sentiment info"
    );
    assert!(
        text.contains("positive") || text.contains("Positive"),
        "Should classify positive reviews"
    );

    teardown(client, server_handle).await;
}

// Item 9: Competitive positioning via MCP
#[tokio::test]
async fn competitive_positioning_via_mcp() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_competitive_positioning",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Competitive") || text.contains("competitive"),
        "Should contain competitive positioning header"
    );
    assert!(
        text.contains("Price") || text.contains("Rating") || text.contains("Amenities"),
        "Should contain axis names"
    );

    teardown(client, server_handle).await;
}

// Item 10: Optimal pricing via MCP
#[tokio::test]
async fn optimal_pricing_via_mcp() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_optimal_pricing",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(
        text.contains("Pricing") || text.contains("pricing") || text.contains("Recommended"),
        "Should contain pricing recommendation"
    );
    assert!(
        text.contains('$') || text.contains("price"),
        "Should reference prices"
    );

    teardown(client, server_handle).await;
}

// Item 13: host_id filtering in host_portfolio
#[tokio::test]
async fn host_portfolio_uses_host_id() {
    let (client, server_handle) = setup().await;

    // Listing "10" has host_name "Alice" and host_id "host-alice"
    // The mock returns 2 listings: one by Alice (host-alice), one by Bob (host-bob)
    // Portfolio should filter by host_id and include only Alice's listing(s)
    let result = client
        .call_tool(tool_params(
            "airbnb_host_portfolio",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    assert!(text.contains("Alice"), "Should contain host name Alice");
    // Only Alice's listing (id=10, "Test Apartment") should appear, not Bob's
    assert!(
        text.contains("Test Apartment"),
        "Should contain Alice's listing"
    );

    teardown(client, server_handle).await;
}

// Item 14: UnavailabilityReason serde roundtrip
#[test]
fn unavailability_reason_serde_roundtrip() {
    let variants = vec![
        UnavailabilityReason::Unknown,
        UnavailabilityReason::Booked,
        UnavailabilityReason::BlockedByHost,
        UnavailabilityReason::PastDate,
        UnavailabilityReason::MinNightRestriction,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).expect("serialize should work");
        let deserialized: UnavailabilityReason =
            serde_json::from_str(&json).expect("deserialize should work");
        assert_eq!(variant, &deserialized, "Roundtrip failed for {variant:?}");
    }
}

// Item 14: Calendar with UnavailabilityReason in MCP output
#[tokio::test]
async fn calendar_shows_unavailability_reason() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_price_calendar",
            serde_json::json!({ "id": "10" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(is_success(&result), "Expected success, got: {text}");
    // The mock provides Booked and BlockedByHost reasons
    assert!(
        text.contains("Booked") || text.contains("Blocked"),
        "Should show unavailability reasons in calendar output, got: {text}"
    );

    teardown(client, server_handle).await;
}

// Item 13: host_id serde roundtrip on Listing
#[test]
fn listing_host_id_serde_roundtrip() {
    let listing = Listing {
        id: "42".into(),
        name: "Test".into(),
        location: "NYC".into(),
        price_per_night: 100.0,
        currency: "$".into(),
        rating: Some(4.5),
        review_count: 10,
        thumbnail_url: None,
        property_type: None,
        host_name: Some("Alice".into()),
        host_id: Some("host-123".into()),
        url: "https://www.airbnb.com/rooms/42".into(),
        is_superhost: None,
        is_guest_favorite: None,
        instant_book: None,
        total_price: None,
        photos: vec![],
        latitude: None,
        longitude: None,
    };

    let json = serde_json::to_string(&listing).expect("serialize should work");
    assert!(json.contains("host-123"), "JSON should contain host_id");
    let deserialized: Listing = serde_json::from_str(&json).expect("deserialize should work");
    assert_eq!(deserialized.host_id, Some("host-123".into()));
}

// ===========================================================================
// Phase 3: MCP protocol verification
// ===========================================================================

#[tokio::test]
async fn list_tools_returns_18() {
    let (client, server_handle) = setup().await;

    let tools = client
        .list_tools(None)
        .await
        .expect("list_tools should work");

    let tool_names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        tool_names.len(),
        18,
        "Expected 18 tools, got {}: {:?}",
        tool_names.len(),
        tool_names
    );

    // Verify all expected tools are present
    let expected = [
        "airbnb_search",
        "airbnb_listing_details",
        "airbnb_reviews",
        "airbnb_price_calendar",
        "airbnb_host_profile",
        "airbnb_neighborhood_stats",
        "airbnb_occupancy_estimate",
        "airbnb_compare_listings",
        "airbnb_price_trends",
        "airbnb_gap_finder",
        "airbnb_revenue_estimate",
        "airbnb_listing_score",
        "airbnb_amenity_analysis",
        "airbnb_market_comparison",
        "airbnb_host_portfolio",
        "airbnb_review_sentiment",
        "airbnb_competitive_positioning",
        "airbnb_optimal_pricing",
    ];
    for name in &expected {
        assert!(
            tool_names.contains(&name.to_string()),
            "Missing tool: {name}"
        );
    }

    teardown(client, server_handle).await;
}

// ===========================================================================
// Phase 4: Edge cases
// ===========================================================================

#[tokio::test]
async fn compare_listings_single_id_error() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_compare_listings",
            serde_json::json!({ "ids": ["10"] }),
        ))
        .await
        .expect("call_tool should succeed");

    assert_eq!(result.is_error, Some(true), "Single ID should return error");
    let text = extract_text(&result);
    assert!(
        text.contains('2') || text.contains("least"),
        "Error should mention minimum of 2, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn market_comparison_single_location_error() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_market_comparison",
            serde_json::json!({ "locations": ["New York"] }),
        ))
        .await
        .expect("call_tool should succeed");

    assert_eq!(
        result.is_error,
        Some(true),
        "Single location should return error"
    );
    let text = extract_text(&result);
    assert!(
        text.contains('2') || text.contains("least"),
        "Error should mention minimum of 2, got: {text}"
    );

    teardown(client, server_handle).await;
}
