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
        next_cursor: None,
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
    let cli = parse_cli(&["airbnb", "--json", "market-comparison", "Paris,Lyon"]);
    let out = dispatch(cli, mock()).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(parsed["locations"].as_array().unwrap().len(), 2);
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
