//! MCP protocol tests: a real rmcp client talks to `AirbnbMcpServer` over an
//! in-memory duplex transport (initialize, tools, resources, errors).

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use mcp_airbnb::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::{AirbnbError, Result};
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use async_trait::async_trait;
use rmcp::ServiceError;
use rmcp::model::{ErrorCode, ProtocolVersion, ReadResourceRequestParams, ResourceContents};
use serde_json::json;

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
            currency: Some("$".into()),
            priced_listings: 0,
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
            past_days_excluded: 0,
            blocked_days_excluded: 0,
            currency: "$".into(),
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

const EXPECTED_TOOLS: [&str; 18] = [
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

/// Does `uri` match the RFC 6570 `template`? Covers the two operators the
/// server uses (P4 Task 6): a `{var}` path segment (one non-empty segment)
/// and a trailing `{?a,b}` form-style query, whose keys must be among
/// `a,b` and whose values must be non-empty. Examples:
/// `airbnb://listing/42` matches `airbnb://listing/{id}`;
/// `airbnb://listing/42/calendar?months=3` matches
/// `airbnb://listing/{id}/calendar{?months}`.
fn uri_matches_template(uri: &str, template: &str) -> bool {
    let (template_path, allowed_keys): (&str, Vec<&str>) = match template.split_once("{?") {
        Some((path, query)) => (path, query.trim_end_matches('}').split(',').collect()),
        None => (template, Vec::new()),
    };
    let (uri_path, uri_query) = match uri.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (uri, None),
    };
    let (Some(uri_path), Some(template_path)) = (
        uri_path.strip_prefix("airbnb://"),
        template_path.strip_prefix("airbnb://"),
    ) else {
        return false;
    };
    let segments: Vec<&str> = uri_path.split('/').collect();
    let pattern: Vec<&str> = template_path.split('/').collect();
    let path_matches = segments.len() == pattern.len()
        && segments.iter().zip(&pattern).all(|(segment, part)| {
            if part.starts_with('{') && part.ends_with('}') {
                !segment.is_empty()
            } else {
                segment == part
            }
        });
    let query_matches = uri_query.is_none_or(|query| {
        query.split('&').all(|pair| {
            pair.split_once('=')
                .is_some_and(|(key, value)| allowed_keys.contains(&key) && !value.is_empty())
        })
    });
    path_matches && query_matches
}

#[test]
fn uri_template_matcher_understands_form_style_queries() {
    assert!(uri_matches_template(
        "airbnb://listing/42",
        "airbnb://listing/{id}"
    ));
    assert!(uri_matches_template(
        "airbnb://listing/42/calendar?months=3",
        "airbnb://listing/{id}/calendar{?months}"
    ));
    assert!(uri_matches_template(
        "airbnb://listing/42/calendar",
        "airbnb://listing/{id}/calendar{?months}"
    ));
    assert!(uri_matches_template(
        "airbnb://analysis/revenue?id=42&months=12",
        "airbnb://analysis/revenue{?id,location,months}"
    ));
    assert!(!uri_matches_template(
        "airbnb://listing/42/calendar?cursor=1",
        "airbnb://listing/{id}/calendar{?months}"
    ));
    assert!(!uri_matches_template(
        "airbnb://search/Berlin, Germany/Mitte",
        "airbnb://search/{location}{?checkin,checkout,adults,children,infants,pets,min_price,max_price,property_type,cursor}"
    ));
}

#[tokio::test]
async fn initialize_reports_server_identity_and_capabilities() {
    let conn = common::connect(Arc::new(IntegrationMock)).await;
    let info = conn
        .client
        .peer_info()
        .expect("the server's initialize result is recorded");
    assert_eq!(info.server_info.name, "mcp-airbnb");
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.protocol_version, ProtocolVersion::LATEST);
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability missing"
    );
    assert!(
        info.capabilities.resources.is_some(),
        "resources capability missing"
    );
    conn.shutdown().await;
}

#[tokio::test]
async fn tools_list_exposes_exactly_the_registered_tools() {
    let conn = common::connect(Arc::new(IntegrationMock)).await;
    let tools = conn
        .client
        .list_all_tools()
        .await
        .expect("tools/list should complete");
    let names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
    let unique: BTreeSet<String> = names.iter().cloned().collect();
    assert_eq!(unique.len(), names.len(), "duplicate tool names: {names:?}");
    let expected: BTreeSet<String> = EXPECTED_TOOLS.iter().map(ToString::to_string).collect();
    assert_eq!(unique, expected);
    for tool in &tools {
        assert!(
            tool.description.as_deref().is_some_and(|d| d.len() >= 40),
            "{} needs a real description",
            tool.name
        );
        assert_eq!(
            tool.input_schema
                .get("type")
                .and_then(serde_json::Value::as_str),
            Some("object"),
            "{} input schema must be a JSON object schema",
            tool.name
        );
        let annotations = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
        assert_eq!(
            annotations.read_only_hint,
            Some(true),
            "{} must be read-only",
            tool.name
        );
        assert_eq!(
            annotations.open_world_hint,
            Some(true),
            "{} talks to Airbnb",
            tool.name
        );
    }
    conn.shutdown().await;
}

#[tokio::test]
async fn resources_read_back_what_a_tool_just_returned() {
    let conn = common::connect(Arc::new(IntegrationMock)).await;
    let result = conn
        .client
        .call_tool(common::tool_call(
            "airbnb_listing_details",
            json!({ "id": "101" }),
        ))
        .await
        .expect("tools/call should complete");
    let tool_text = common::text_of(&result);
    assert_ne!(
        result.is_error,
        Some(true),
        "listing details failed: {tool_text}"
    );

    let uri = "airbnb://listing/101";
    let listed = conn
        .client
        .list_all_resources()
        .await
        .expect("resources/list should complete");
    let uris: Vec<&str> = listed
        .iter()
        .map(|resource| resource.uri.as_str())
        .collect();
    assert!(uris.contains(&uri), "{uri} missing from {uris:?}");

    let read = conn
        .client
        .read_resource(ReadResourceRequestParams::new(uri))
        .await
        .expect("resources/read should complete");
    assert_eq!(read.contents.len(), 1, "one content block per resource");
    match &read.contents[0] {
        ResourceContents::TextResourceContents {
            uri: got,
            mime_type,
            text,
            ..
        } => {
            assert_eq!(got, uri);
            assert_eq!(
                mime_type.as_deref(),
                Some("text/plain"),
                "P4 (MCP-12) serves text/plain"
            );
            assert_eq!(
                text, &tool_text,
                "the resource must hold exactly what the tool returned"
            );
        }
        other @ ResourceContents::BlobResourceContents { .. } => {
            panic!("expected text contents, got {other:?}")
        }
    }
    conn.shutdown().await;
}

#[tokio::test]
async fn reading_an_unknown_resource_is_resource_not_found() {
    let conn = common::connect(Arc::new(IntegrationMock)).await;
    let err = conn
        .client
        .read_resource(ReadResourceRequestParams::new("airbnb://listing/999"))
        .await
        .expect_err("an unknown URI must fail");
    match err {
        ServiceError::McpError(data) => assert_eq!(data.code, ErrorCode::RESOURCE_NOT_FOUND),
        other => panic!("expected an MCP error, got {other:?}"),
    }
    conn.shutdown().await;
}

#[tokio::test]
async fn every_stored_resource_uri_matches_an_advertised_template() {
    let conn = common::connect(Arc::new(IntegrationMock)).await;
    let calls = [
        (
            "airbnb_search",
            json!({ "location": "Berlin, Germany / Mitte" }),
        ),
        ("airbnb_listing_details", json!({ "id": "101" })),
        ("airbnb_reviews", json!({ "id": "101" })),
        ("airbnb_price_calendar", json!({ "id": "101", "months": 1 })),
        ("airbnb_host_profile", json!({ "id": "101" })),
        (
            "airbnb_neighborhood_stats",
            json!({ "location": "Berlin, Germany" }),
        ),
        (
            "airbnb_occupancy_estimate",
            json!({ "id": "101", "months": 1 }),
        ),
        (
            "airbnb_market_comparison",
            json!({ "locations": ["Berlin, Germany", "Munich / Bavaria"] }),
        ),
    ];
    let call_count = calls.len();
    for (name, args) in calls {
        let result = conn
            .client
            .call_tool(common::tool_call(name, args))
            .await
            .expect("tools/call should complete");
        assert_ne!(
            result.is_error,
            Some(true),
            "{name} failed: {}",
            common::text_of(&result)
        );
    }
    let templates: Vec<String> = conn
        .client
        .list_all_resource_templates()
        .await
        .expect("resources/templates/list should complete")
        .into_iter()
        .map(|template| template.raw.uri_template)
        .collect();
    let resources = conn
        .client
        .list_all_resources()
        .await
        .expect("resources/list should complete");
    assert!(
        resources.len() >= call_count,
        "each successful call stores a resource, got {}",
        resources.len()
    );
    assert_eq!(templates.len(), 18, "P4 advertises one template per tool");
    for resource in &resources {
        let uri = resource.uri.as_str();
        assert!(
            !uri.contains(char::is_whitespace) && !uri.contains('#'),
            "resource URI is not encoded: {uri}"
        );
        assert!(
            templates
                .iter()
                .any(|template| uri_matches_template(uri, template)),
            "{uri} matches none of {templates:?}"
        );
    }
    conn.shutdown().await;
}

#[tokio::test]
async fn upstream_errors_surface_as_tool_errors_and_store_nothing() {
    let conn = common::connect(Arc::new(ErrorMock)).await;
    let result = conn
        .client
        .call_tool(common::tool_call(
            "airbnb_listing_details",
            json!({ "id": "101" }),
        ))
        .await
        .expect("a failing tool is still a successful JSON-RPC call");
    assert_eq!(result.is_error, Some(true));
    let text = common::text_of(&result);
    assert!(
        text.contains("101"),
        "the error must name the listing: {text}"
    );
    let resources = conn
        .client
        .list_all_resources()
        .await
        .expect("resources/list should complete");
    assert!(
        resources.is_empty(),
        "failed calls must not create resources: {:?}",
        resources.iter().map(|r| r.uri.as_str()).collect::<Vec<_>>()
    );
    conn.shutdown().await;
}
