//! ARCH-1 parity: every analytical tool must run the same orchestration
//! from the CLI and from MCP. Each case runs one fresh recording mock
//! through `cli::dispatch` and another through an in-process MCP server,
//! then compares the ordered list of upstream calls.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use clap::Parser;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo};
use rmcp::{ClientHandler, ServiceExt};

use mcp_airbnb::cli::{Cli, dispatch};
use mcp_airbnb::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::Result;
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

#[derive(Default)]
struct RecordingMock {
    calls: Mutex<Vec<String>>,
}

impl RecordingMock {
    fn record(&self, call: String) {
        self.calls.lock().expect("calls lock").push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }
}

fn listing(id: &str) -> Listing {
    Listing {
        id: id.into(),
        name: format!("Flat {id}"),
        location: "Rome, Italy".into(),
        price_per_night: 150.0,
        currency: "€".into(),
        rating: Some(4.7),
        review_count: 20,
        thumbnail_url: None,
        property_type: Some("Apartment".into()),
        host_name: None,
        host_id: None,
        url: format!("https://www.airbnb.com/rooms/{id}"),
        is_superhost: None,
        is_guest_favorite: None,
        instant_book: None,
        total_price: None,
        photos: vec![],
        latitude: None,
        longitude: None,
    }
}

fn detail(id: &str) -> ListingDetail {
    ListingDetail {
        id: id.into(),
        name: format!("Flat {id}"),
        location: "Rome, Italy".into(),
        description: "Bright flat.".into(),
        price_per_night: 150.0,
        currency: "€".into(),
        rating: Some(4.7),
        review_count: 20,
        property_type: Some("Apartment".into()),
        host_name: None,
        url: format!("https://www.airbnb.com/rooms/{id}"),
        amenities: vec!["Wifi".into(), "Kitchen".into()],
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
    }
}

#[async_trait]
impl AirbnbClient for RecordingMock {
    async fn search_listings(&self, p: &SearchParams) -> Result<SearchResult> {
        self.record(format!(
            "search location={} checkin={:?} checkout={:?} adults={:?} property_type={:?} cursor={:?}",
            p.location, p.checkin, p.checkout, p.adults, p.property_type, p.cursor
        ));
        Ok(SearchResult {
            listings: vec![listing("101"), listing("102"), listing("103")],
            total_count: Some(3),
            next_cursor: Some("page-2".into()),
        })
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        self.record(format!("detail {id}"));
        Ok(detail(id))
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        self.record(format!("reviews {id} cursor={cursor:?}"));
        let next_cursor = match cursor {
            None => Some("1".to_string()),
            Some("1") => Some("2".to_string()),
            Some(_) => None,
        };
        Ok(ReviewsPage {
            listing_id: id.into(),
            summary: None,
            reviews: vec![Review {
                author: "Guest A".into(),
                date: "2026-08-01".into(),
                rating: Some(5.0),
                comment: "Clean and quiet, great location.".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            }],
            next_cursor,
        })
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        self.record(format!("calendar {id} months={months}"));
        Ok(PriceCalendar {
            listing_id: id.into(),
            currency: "€".into(),
            days: (1..=28)
                .map(|d| CalendarDay {
                    date: format!("2026-10-{d:02}"),
                    price: None,
                    available: d % 3 != 0,
                    min_nights: Some(2),
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                })
                .collect(),
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        })
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        self.record(format!("host {listing_id}"));
        Ok(HostProfile {
            host_id: None,
            name: "Host A".into(),
            is_superhost: None,
            response_rate: None,
            response_time: None,
            member_since: None,
            languages: vec![],
            total_listings: None,
            description: None,
            profile_picture_url: None,
            identity_verified: None,
        })
    }

    async fn get_neighborhood_stats(&self, p: &SearchParams) -> Result<NeighborhoodStats> {
        self.record(format!(
            "neighborhood location={} checkin={:?} checkout={:?} property_type={:?}",
            p.location, p.checkin, p.checkout, p.property_type
        ));
        Ok(NeighborhoodStats {
            location: p.location.clone(),
            total_listings: 40,
            average_price: Some(140.0),
            median_price: Some(130.0),
            price_range: Some((80.0, 300.0)),
            average_rating: Some(4.6),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("€".into()),
            priced_listings: 40,
        })
    }

    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate> {
        self.record(format!("occupancy {id} months={months}"));
        Ok(OccupancyEstimate {
            listing_id: id.into(),
            period_start: "2026-10-01".into(),
            period_end: "2026-12-31".into(),
            total_days: 92,
            occupied_days: 46,
            available_days: 46,
            occupancy_rate: 50.0,
            past_days_excluded: 0,
            blocked_days_excluded: 0,
            currency: "€".into(),
            average_available_price: None,
            weekend_avg_price: None,
            weekday_avg_price: None,
            monthly_breakdown: vec![],
        })
    }
}

#[derive(Clone, Default)]
struct QuietClient;

impl ClientHandler for QuietClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

async fn run_cli(args: &[&str]) -> (anyhow::Result<String>, Vec<String>) {
    let mock = Arc::new(RecordingMock::default());
    let cli = Cli::try_parse_from(args).expect("cli parses");
    let out = dispatch(cli, Arc::clone(&mock) as Arc<dyn AirbnbClient>).await;
    (out, mock.calls())
}

async fn run_mcp(tool: &str, args: serde_json::Value) -> (CallToolResult, Vec<String>) {
    let mock = Arc::new(RecordingMock::default());
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    let server = AirbnbMcpServer::new(Arc::clone(&mock) as Arc<dyn AirbnbClient>);
    let handle = tokio::spawn(async move {
        server.serve(server_io).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = QuietClient.serve(client_io).await.expect("client connects");
    let serde_json::Value::Object(arguments) = args else {
        panic!("tool arguments must be a JSON object");
    };
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(arguments))
        .await
        .expect("tools/call transport succeeds");
    let _ = client.cancel().await;
    let _ = handle.await;
    (result, mock.calls())
}

const CASES: &[(&[&str], &str, &str)] = &[
    (
        &["airbnb", "price-trends", "42"],
        "airbnb_price_trends",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "gap-finder", "42"],
        "airbnb_gap_finder",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "revenue", "42"],
        "airbnb_revenue_estimate",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "revenue", "--location", "Rome, Italy"],
        "airbnb_revenue_estimate",
        r#"{"location":"Rome, Italy"}"#,
    ),
    (
        &["airbnb", "listing-score", "42"],
        "airbnb_listing_score",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "amenities", "42"],
        "airbnb_amenity_analysis",
        r#"{"id":"42"}"#,
    ),
    (
        &[
            "airbnb",
            "market-comparison",
            "Paris, France",
            "Lyon, France",
        ],
        "airbnb_market_comparison",
        r#"{"locations":["Paris, France","Lyon, France"]}"#,
    ),
    (
        &["airbnb", "host-portfolio", "42"],
        "airbnb_host_portfolio",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "review-sentiment", "42"],
        "airbnb_review_sentiment",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "competitive", "42"],
        "airbnb_competitive_positioning",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "optimal-pricing", "42"],
        "airbnb_optimal_pricing",
        r#"{"id":"42"}"#,
    ),
    (
        &["airbnb", "compare", "--ids", "101,102"],
        "airbnb_compare_listings",
        r#"{"ids":["101","102"]}"#,
    ),
    (
        &["airbnb", "compare", "--location", "Rome, Italy"],
        "airbnb_compare_listings",
        r#"{"location":"Rome, Italy"}"#,
    ),
    (
        &["airbnb", "search", "Rome, Italy"],
        "airbnb_search",
        r#"{"location":"Rome, Italy"}"#,
    ),
];

#[tokio::test]
async fn cli_and_mcp_make_identical_upstream_calls() {
    for (cli_args, tool, json_args) in CASES {
        let (cli_out, cli_calls) = run_cli(cli_args).await;
        let (mcp_out, mcp_calls) =
            run_mcp(tool, serde_json::from_str(json_args).expect("valid json")).await;
        assert!(cli_out.is_ok(), "{tool}: CLI failed: {:?}", cli_out.err());
        assert_ne!(
            mcp_out.is_error,
            Some(true),
            "{tool}: MCP failed: {mcp_out:?}"
        );
        assert!(!cli_calls.is_empty(), "{tool}: no upstream call recorded");
        assert_eq!(
            cli_calls, mcp_calls,
            "{tool}: CLI {cli_args:?} and MCP {json_args} diverged"
        );
    }
}

#[tokio::test]
async fn invalid_listing_ids_fail_on_both_entry_points_without_fetching() {
    let cases: [(&[&str], &str, &str); 3] = [
        (
            &["airbnb", "revenue", "abc"],
            "airbnb_revenue_estimate",
            r#"{"id":"abc"}"#,
        ),
        (
            &["airbnb", "listing-score", "0042"],
            "airbnb_listing_score",
            r#"{"id":"0042"}"#,
        ),
        (
            &["airbnb", "optimal-pricing", "abc"],
            "airbnb_optimal_pricing",
            r#"{"id":"abc"}"#,
        ),
    ];
    for (cli_args, tool, json_args) in cases {
        let (cli_out, cli_calls) = run_cli(cli_args).await;
        let (mcp_out, mcp_calls) =
            run_mcp(tool, serde_json::from_str(json_args).expect("valid json")).await;
        assert!(cli_out.is_err(), "{tool}: CLI accepted {cli_args:?}");
        assert_eq!(
            mcp_out.is_error,
            Some(true),
            "{tool}: MCP accepted {json_args}"
        );
        assert!(
            cli_calls.is_empty() && mcp_calls.is_empty(),
            "{tool}: fetched anyway"
        );
    }
}

#[tokio::test]
async fn out_of_range_input_is_rejected_by_both_entry_points() {
    assert!(Cli::try_parse_from(["airbnb", "price-trends", "42", "--months", "13"]).is_err());
    let (mcp_out, calls) = run_mcp(
        "airbnb_price_trends",
        serde_json::json!({"id":"42","months":13}),
    )
    .await;
    assert_eq!(mcp_out.is_error, Some(true));
    assert!(calls.is_empty());

    assert!(
        Cli::try_parse_from(["airbnb", "market-comparison", "A", "B", "C", "D", "E", "F"]).is_err()
    );
    let (mcp_out, calls) = run_mcp(
        "airbnb_market_comparison",
        serde_json::json!({"locations":["A","B","C","D","E","F"]}),
    )
    .await;
    assert_eq!(mcp_out.is_error, Some(true));
    assert!(calls.is_empty());
}

#[tokio::test]
async fn invalid_location_overrides_fail_on_both_entry_points_without_fetching() {
    // I8: the optional `location` of revenue, amenities, competitive and
    // optimal pricing follows the same rule on the CLI as on MCP.
    let too_long = "a".repeat(201);
    let cases: [(&str, &str, &str); 4] = [
        ("revenue", "airbnb_revenue_estimate", ""),
        (
            "optimal-pricing",
            "airbnb_optimal_pricing",
            too_long.as_str(),
        ),
        ("amenities", "airbnb_amenity_analysis", "Paris\nINFO forged"),
        ("competitive", "airbnb_competitive_positioning", "../.."),
    ];
    for (command, tool, location) in cases {
        let (cli_out, cli_calls) =
            run_cli(&["airbnb", command, "42", "--location", location]).await;
        let (mcp_out, mcp_calls) =
            run_mcp(tool, serde_json::json!({"id":"42","location":location})).await;
        let err = cli_out.expect_err(&format!("{command}: CLI accepted {location:?}"));
        let report = mcp_airbnb::cli::render_error(&err, true);
        assert_eq!(
            report.exit_code,
            mcp_airbnb::cli::EXIT_INVALID_INPUT,
            "{command}: {}",
            report.output
        );
        assert!(
            report.output.contains("\"invalid_params\""),
            "{command}: {}",
            report.output
        );
        assert_eq!(
            mcp_out.is_error,
            Some(true),
            "{tool}: MCP accepted {location:?}"
        );
        assert!(
            cli_calls.is_empty() && mcp_calls.is_empty(),
            "{command}: fetched anyway (CLI {cli_calls:?}, MCP {mcp_calls:?})"
        );
    }
}

#[tokio::test]
async fn host_portfolio_without_host_identity_keeps_only_the_anchor_listing() {
    // Detail and search results carry neither host_id nor host_name
    // (P2 carry-over: MATH-11, BUG-6, MCP-6, GAP-9).
    let (cli_out, _) = run_cli(&["airbnb", "--json", "host-portfolio", "42"]).await;
    let parsed: serde_json::Value =
        serde_json::from_str(cli_out.expect("cli ok").trim()).expect("json");
    assert_eq!(parsed["total_properties"], 1);
    assert_eq!(parsed["properties"][0]["id"], "42");
    let (mcp_out, _) = run_mcp("airbnb_host_portfolio", serde_json::json!({"id":"42"})).await;
    assert_ne!(mcp_out.is_error, Some(true));
}

#[tokio::test]
async fn compare_by_location_never_counts_a_listing_twice() {
    // The mock returns the same three listings on every page (P1a carry-over: MCP-1).
    let (cli_out, cli_calls) = run_cli(&[
        "airbnb",
        "--json",
        "compare",
        "--location",
        "Rome, Italy",
        "--max-listings",
        "60",
    ])
    .await;
    let parsed: serde_json::Value =
        serde_json::from_str(cli_out.expect("cli ok").trim()).expect("json");
    assert_eq!(parsed["listings"].as_array().expect("listings").len(), 3);
    let (_, mcp_calls) = run_mcp(
        "airbnb_compare_listings",
        serde_json::json!({"location":"Rome, Italy","max_listings":60}),
    )
    .await;
    assert_eq!(cli_calls, mcp_calls);
}

/// Source text before the file's top-level `#[cfg(test)] mod tests`. Split on
/// the unindented marker only: `src/mcp/server.rs` also has an indented
/// `#[cfg(test)]` helper inside `impl ResourceStore` (Task 9), and splitting
/// there would hide every handler from the guard.
fn production_part(source: &str) -> &str {
    source
        .split("\n#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or(source)
}

#[test]
fn mcp_server_has_no_orchestration_of_its_own() {
    let production = production_part(include_str!("../src/mcp/server.rs"));
    assert!(
        production.contains("async fn airbnb_optimal_pricing"),
        "the guard must see the handlers"
    );
    // Match the bare `compute_` prefix so a braced import
    // (`analytics::{compute_…}`) cannot slip past the guard.
    assert!(
        !production.contains("compute_"),
        "MCP handlers must delegate to application::analytical_handlers"
    );
}

#[test]
fn cli_dispatch_has_no_orchestration_of_its_own() {
    let production = production_part(include_str!("../src/cli/mod.rs"));
    assert!(
        production.contains("pub async fn dispatch"),
        "the guard must see dispatch"
    );
    assert!(
        !production.contains("domain::analytics"),
        "CLI analytical commands must call application::analytical_handlers"
    );
}
