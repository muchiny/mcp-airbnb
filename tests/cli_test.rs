//! Integration tests for the `airbnb` CLI dispatcher.
//!
//! These tests exercise `mcp_airbnb::cli::dispatch(cli, Arc<dyn AirbnbClient>)`
//! against an inline mock client, covering both the text and JSON output
//! paths plus a few analytical multi-fetch commands. The mock pattern
//! mirrors the existing `tests/mcp_server_test.rs`.

use std::sync::Arc;

use async_trait::async_trait;
use clap::Parser;

use mcp_airbnb::cli::{Cli, dispatch};
use mcp_airbnb::domain::analytics::{
    HostProfile, MonthlyOccupancy, NeighborhoodStats, OccupancyEstimate, PropertyTypeCount,
};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::Result;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

// ---------------- Inline mock ----------------

struct CliMock;

fn sample_listing(id: &str, name: &str, price: f64) -> Listing {
    Listing {
        id: id.into(),
        name: name.into(),
        location: "Rome, Italy".into(),
        price_per_night: price,
        currency: "€".into(),
        rating: Some(4.7),
        review_count: 42,
        thumbnail_url: None,
        property_type: Some("Apartment".into()),
        host_name: Some("Marco".into()),
        host_id: Some("host-1".into()),
        url: format!("https://www.airbnb.com/rooms/{id}"),
        is_superhost: Some(true),
        is_guest_favorite: None,
        instant_book: None,
        total_price: None,
        photos: vec![],
        latitude: None,
        longitude: None,
    }
}

fn sample_detail(id: &str) -> ListingDetail {
    ListingDetail {
        id: id.into(),
        name: "Sunny Loft".into(),
        location: "Rome, Italy".into(),
        description: "A beautiful place".repeat(50),
        price_per_night: 150.0,
        currency: "€".into(),
        rating: Some(4.85),
        review_count: 75,
        property_type: Some("Apartment".into()),
        host_name: Some("Marco".into()),
        url: format!("https://www.airbnb.com/rooms/{id}"),
        amenities: vec![
            "WiFi".into(),
            "Kitchen".into(),
            "Heating".into(),
            "Washer".into(),
            "TV".into(),
            "Iron".into(),
        ],
        house_rules: vec!["No smoking".into()],
        latitude: Some(41.9),
        longitude: Some(12.5),
        photos: (0..25).map(|i| format!("photo{i}.jpg")).collect(),
        bedrooms: Some(2),
        beds: Some(3),
        bathrooms: Some(1.5),
        max_guests: Some(4),
        check_in_time: Some("15:00".into()),
        check_out_time: Some("11:00".into()),
        host_id: Some("host-1".into()),
        host_is_superhost: Some(true),
        host_response_rate: Some("100%".into()),
        host_response_time: Some("within an hour".into()),
        host_joined: Some("2018".into()),
        host_total_listings: Some(3),
        host_languages: vec!["English".into(), "Italian".into()],
        cancellation_policy: Some("Flexible".into()),
        instant_book: Some(true),
        cleaning_fee: Some(40.0),
        service_fee: Some(20.0),
        neighborhood: Some("Trastevere".into()),
    }
}

fn sample_calendar(id: &str) -> PriceCalendar {
    let days = (1..=90)
        .map(|d| CalendarDay {
            date: format!("2025-06-{:02}", ((d - 1) % 28) + 1),
            price: Some(150.0 + f64::from(d % 7) * 10.0),
            available: d % 5 != 0,
            min_nights: Some(2),
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
            unavailability_reason: None,
        })
        .collect();
    PriceCalendar {
        listing_id: id.into(),
        currency: "€".into(),
        days,
        average_price: Some(165.0),
        occupancy_rate: Some(80.0),
        min_price: Some(150.0),
        max_price: Some(210.0),
    }
}

fn sample_neighborhood(location: &str) -> NeighborhoodStats {
    NeighborhoodStats {
        location: location.into(),
        total_listings: 100,
        average_price: Some(140.0),
        median_price: Some(130.0),
        price_range: Some((80.0, 300.0)),
        average_rating: Some(4.6),
        property_type_distribution: vec![PropertyTypeCount {
            property_type: "Apartment".into(),
            count: 60,
            percentage: 60.0,
        }],
        superhost_percentage: Some(40.0),
        currency: Some("$".into()),
        priced_listings: 0,
    }
}

fn sample_host_profile() -> HostProfile {
    HostProfile {
        host_id: Some("host-1".into()),
        name: "Marco".into(),
        is_superhost: Some(true),
        response_rate: Some("100%".into()),
        response_time: Some("within an hour".into()),
        member_since: Some("2018".into()),
        languages: vec!["English".into(), "Italian".into()],
        total_listings: Some(3),
        description: Some("Experienced host".into()),
        profile_picture_url: None,
        identity_verified: Some(true),
    }
}

fn sample_occupancy(id: &str) -> OccupancyEstimate {
    OccupancyEstimate {
        listing_id: id.into(),
        period_start: "2025-06-01".into(),
        period_end: "2025-08-31".into(),
        total_days: 90,
        occupied_days: 72,
        available_days: 18,
        occupancy_rate: 80.0,
        past_days_excluded: 0,
        blocked_days_excluded: 0,
        currency: "$".into(),
        average_available_price: Some(165.0),
        weekend_avg_price: Some(185.0),
        weekday_avg_price: Some(155.0),
        monthly_breakdown: vec![MonthlyOccupancy {
            month: "2025-06".into(),
            total_days: 30,
            occupied_days: 24,
            available_days: 6,
            occupancy_rate: 80.0,
            average_price: Some(160.0),
        }],
    }
}

fn sample_reviews(id: &str) -> ReviewsPage {
    ReviewsPage {
        listing_id: id.into(),
        summary: None,
        reviews: vec![
            Review {
                author: "Alice".into(),
                date: "2025-05-01".into(),
                rating: Some(5.0),
                comment: "Absolutely amazing stay! Super clean and beautiful location.".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            },
            Review {
                author: "Bob".into(),
                date: "2025-04-15".into(),
                rating: Some(4.0),
                comment: "Great host, comfortable place, would recommend.".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            },
        ],
        next_cursor: Some("24".into()),
    }
}

#[async_trait]
impl AirbnbClient for CliMock {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        Ok(SearchResult {
            listings: vec![
                sample_listing("101", "Sunny Loft", 150.0),
                sample_listing("102", "Cozy Studio", 110.0),
                sample_listing("103", "Rooftop Flat", 200.0),
            ],
            total_count: Some(3),
            next_cursor: None,
        })
        .map(|mut r| {
            for l in &mut r.listings {
                l.location.clone_from(&params.location);
            }
            r
        })
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        Ok(sample_detail(id))
    }

    async fn get_reviews(&self, id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Ok(sample_reviews(id))
    }

    async fn get_price_calendar(&self, id: &str, _months: u32) -> Result<PriceCalendar> {
        Ok(sample_calendar(id))
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Ok(sample_host_profile())
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        Ok(sample_neighborhood(&params.location))
    }

    async fn get_occupancy_estimate(&self, id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Ok(sample_occupancy(id))
    }
}

fn mock() -> Arc<dyn AirbnbClient> {
    Arc::new(CliMock)
}

fn parse_cli(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).expect("cli should parse")
}

// ---------------- Data tools ----------------

#[tokio::test]
async fn search_text_output_contains_listing_name() {
    let cli = parse_cli(&["airbnb", "search", "Rome, Italy"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Sunny Loft"), "output: {out}");
    assert!(out.contains("Rome, Italy"));
}

#[tokio::test]
async fn search_json_output_is_valid_json() {
    let cli = parse_cli(&["airbnb", "--json", "search", "Rome, Italy"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).expect("valid JSON");
    assert_eq!(parsed["listings"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["listings"][0]["name"], "Sunny Loft");
}

#[tokio::test]
async fn search_text_output_shows_ids_and_urls() {
    let cli = parse_cli(&["airbnb", "search", "Rome, Italy"]);
    let out = dispatch(cli, mock()).await.unwrap();
    for id in ["101", "102", "103"] {
        assert!(out.contains(&format!("[ID {id}]")), "output: {out}");
        assert!(
            out.contains(&format!("https://www.airbnb.com/rooms/{id}")),
            "output: {out}"
        );
    }
}

#[tokio::test]
async fn listing_details_returns_full_detail() {
    let cli = parse_cli(&["airbnb", "listing", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Sunny Loft"));
    assert!(out.contains("Location: Rome, Italy"));
}

#[tokio::test]
async fn listing_details_json_includes_id() {
    let cli = parse_cli(&["airbnb", "--json", "listing", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["id"], "12345");
}

#[tokio::test]
async fn neighborhood_stats_text() {
    let cli = parse_cli(&["airbnb", "neighborhood", "Rome"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Rome"));
}

#[tokio::test]
async fn calendar_text_contains_days() {
    let cli = parse_cli(&["airbnb", "calendar", "12345", "--months", "3"]);
    let out = dispatch(cli, mock()).await.unwrap();
    // PriceCalendar Display should show the listing id
    assert!(out.contains("12345"));
}

#[tokio::test]
async fn occupancy_text_contains_rate() {
    let cli = parse_cli(&["airbnb", "occupancy", "12345", "--months", "3"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("80"));
}

#[tokio::test]
async fn reviews_text_contains_author() {
    let cli = parse_cli(&["airbnb", "reviews", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Alice") || out.contains("Bob"));
}

#[tokio::test]
async fn reviews_text_shows_next_cursor() {
    let cli = parse_cli(&["airbnb", "reviews", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Next page cursor: 24"), "output: {out}");
}

#[tokio::test]
async fn host_profile_text_contains_name() {
    let cli = parse_cli(&["airbnb", "host", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(out.contains("Marco"));
}

// ---------------- Analytical: single-fetch ----------------

#[tokio::test]
async fn price_trends_computes_from_calendar() {
    let cli = parse_cli(&["airbnb", "--json", "price-trends", "12345", "--months", "3"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn gap_finder_json_output() {
    let cli = parse_cli(&["airbnb", "--json", "gap-finder", "12345", "--months", "3"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn listing_score_returns_json() {
    let cli = parse_cli(&["airbnb", "--json", "listing-score", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
    assert!(parsed["overall_score"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn review_sentiment_returns_json() {
    let cli = parse_cli(&[
        "airbnb",
        "--json",
        "review-sentiment",
        "12345",
        "--max-pages",
        "1",
    ]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn market_comparison_two_locations() {
    let cli = parse_cli(&["airbnb", "--json", "market-comparison", "Paris", "Lyon"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["locations"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn market_comparison_city_country_pairs() {
    let cli = parse_cli(&[
        "airbnb",
        "--json",
        "market-comparison",
        "Paris, France",
        "Barcelona, Spain",
    ]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["locations"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["locations"][0]["location"], "Paris, France");
    assert_eq!(parsed["locations"][1]["location"], "Barcelona, Spain");
}

// ---------------- Analytical: multi-fetch ----------------

#[tokio::test]
async fn compare_via_location() {
    let cli = parse_cli(&["airbnb", "--json", "compare", "--location", "Rome"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(parsed["listings"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn compare_via_ids() {
    let cli = parse_cli(&["airbnb", "--json", "compare", "--ids", "101,102"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(parsed["listings"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn compare_via_ids_reports_bedrooms_and_amenities() {
    let cli = parse_cli(&["airbnb", "--json", "compare", "--ids", "101,102"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    // `sample_detail` has 2 bedrooms and 6 amenities.
    assert_eq!(parsed["listings"][0]["bedrooms"], 2);
    assert_eq!(parsed["listings"][0]["amenities_count"], 6);
}

#[tokio::test]
async fn compare_rejects_when_neither_ids_nor_location() {
    let cli = parse_cli(&["airbnb", "compare"]);
    let result = dispatch(cli, mock()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn revenue_estimate_returns_json() {
    let cli = parse_cli(&[
        "airbnb",
        "--json",
        "revenue",
        "12345",
        "--location",
        "Rome",
        "--months",
        "6",
    ]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn amenities_analysis_returns_json() {
    let cli = parse_cli(&["airbnb", "--json", "amenities", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn host_portfolio_returns_json() {
    let cli = parse_cli(&["airbnb", "--json", "host-portfolio", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["host_name"], "Marco");
}

#[tokio::test]
async fn competitive_positioning_returns_json() {
    let cli = parse_cli(&["airbnb", "--json", "competitive", "12345"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn optimal_pricing_returns_json() {
    let cli = parse_cli(&[
        "airbnb",
        "--json",
        "optimal-pricing",
        "12345",
        "--months",
        "6",
    ]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["listing_id"], "12345");
}

#[tokio::test]
async fn revenue_location_only_returns_json() {
    let cli = parse_cli(&["airbnb", "--json", "revenue", "--location", "Rome, Italy"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(parsed["listing_id"].is_null());
    assert_eq!(parsed["location"], "Rome, Italy");
}

#[tokio::test]
async fn compare_via_location_reports_pages() {
    let cli = parse_cli(&["airbnb", "compare", "--location", "Rome"]);
    let out = dispatch(cli, mock()).await.unwrap();
    assert!(
        out.contains("Fetched 3 listings across 1 page(s)."),
        "output: {out}"
    );
}

fn airbnb_binary(args: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = dir.path().join("config.yaml");
    std::fs::write(&config, "scraper:\n  graphql_enabled: false\n").expect("write config");
    let config_arg = config.to_str().expect("utf-8 path").to_string();
    let mut full: Vec<&str> = vec!["--config", config_arg.as_str()];
    full.extend_from_slice(args);
    std::process::Command::new(env!("CARGO_BIN_EXE_airbnb"))
        .args(&full)
        .env_remove("AIRBNB_CONFIG")
        .env("RUST_LOG", "off")
        .output()
        .expect("run airbnb")
}

#[test]
fn json_mode_reports_errors_as_json_with_exit_code_2() {
    let output = airbnb_binary(&["--json", "listing", "abc"]);
    assert_eq!(output.status.code(), Some(2));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(body["error"]["kind"], "invalid_params");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("abc")
    );
}

#[test]
fn text_mode_reports_errors_on_stderr_with_exit_code_2() {
    let output = airbnb_binary(&["listing", "abc"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error:"));
}

#[test]
fn json_mode_reports_clap_range_errors_as_json_with_exit_code_2() {
    // CLI-2: ranges enforced by clap value parsers must reach `| jq` as JSON too.
    let cases: [&[&str]; 4] = [
        &["--json", "price-trends", "42", "--months", "13"],
        &["price-trends", "42", "--months", "13", "-j"],
        &["-vj", "review-sentiment", "42", "--max-pages", "21"],
        &["--json", "market-comparison", "Paris"],
    ];
    for args in cases {
        let output = airbnb_binary(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let body: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: stdout is not JSON ({e})"));
        assert_eq!(body["error"]["kind"], "invalid_params", "{args:?}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.starts_with("Invalid parameters: "), "{message}");
        assert!(!message.contains("error: "), "{message}");
        assert!(!message.contains('\n'), "{message}");
    }
}

#[test]
fn text_mode_keeps_clap_usage_errors_on_stderr() {
    let output = airbnb_binary(&["price-trends", "42", "--months", "13"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--months"), "{stderr}");
}

#[test]
fn json_mode_keeps_help_and_version_as_plain_text() {
    let help = airbnb_binary(&["--json", "--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));

    let version = airbnb_binary(&["--json", "--version"]);
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("airbnb "));
}
