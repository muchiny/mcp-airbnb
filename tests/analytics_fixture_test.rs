//! End-to-end analytics over a real Airbnb `PdpAvailabilityCalendar`
//! payload captured on 2026-09-28. The payload is anonymized: listing id
//! replaced, trace id and condition ranges removed. The reference date is
//! pinned so the expectations never drift with the clock.

use chrono::NaiveDate;

use mcp_airbnb::adapters::scraper::calendar_parser::parse_price_calendar;
use mcp_airbnb::domain::analytics::{
    DataSource, NeighborhoodStats, compute_gap_finder, compute_occupancy_estimate,
    compute_price_trends, compute_revenue_estimate,
};
use mcp_airbnb::domain::calendar::{PriceCalendar, UnavailabilityReason};

const LIVE_CALENDAR: &str =
    include_str!("fixtures/airbnb/2026-09/analytics/pdp_availability_calendar_2026-09-28.json");

fn live_calendar() -> PriceCalendar {
    let mut calendar =
        parse_price_calendar(LIVE_CALENDAR, "12345678").expect("live calendar parses");
    calendar.classify_past_days(NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid date"));
    calendar
}

#[test]
fn live_calendar_has_no_published_prices() {
    let calendar = live_calendar();
    assert_eq!(calendar.days.len(), 91);
    assert!(calendar.days.iter().all(|d| d.price.is_none()));
}

#[test]
fn live_calendar_occupancy_excludes_the_27_past_days() {
    let calendar = live_calendar();
    let past = calendar
        .days
        .iter()
        .filter(|d| d.unavailability_reason == Some(UnavailabilityReason::PastDate))
        .count();
    assert_eq!(past, 27);

    let est = compute_occupancy_estimate("12345678", &calendar);
    assert_eq!(est.past_days_excluded, 27);
    assert_eq!(est.total_days, 64);
    assert_eq!(est.occupied_days, 53);
    let expected = 53.0 / 64.0 * 100.0;
    assert!(
        (est.occupancy_rate - expected).abs() < 1e-9,
        "got {}",
        est.occupancy_rate
    );
    let months: Vec<(&str, u32, u32)> = est
        .monthly_breakdown
        .iter()
        .map(|m| (m.month.as_str(), m.total_days, m.occupied_days))
        .collect();
    assert_eq!(
        months,
        vec![("2026-09", 3, 3), ("2026-10", 31, 29), ("2026-11", 30, 21)]
    );
}

#[test]
fn live_calendar_price_trends_report_no_prices() {
    let trends = compute_price_trends("12345678", &live_calendar());
    assert!(trends.overall_avg.is_none());
    assert_eq!(trends.priced_nights, 0);
    let months: Vec<(&str, u32, u32)> = trends
        .monthly
        .iter()
        .map(|m| (m.month.as_str(), m.available_days, m.total_days))
        .collect();
    assert_eq!(
        months,
        vec![("2026-09", 0, 3), ("2026-10", 2, 31), ("2026-11", 9, 30)]
    );
    assert!(trends.to_string().contains("Nightly prices: not published"));
}

#[test]
fn live_calendar_gap_finder_finds_only_short_gaps() {
    let result = compute_gap_finder("12345678", &live_calendar());
    assert_eq!(result.total_gaps, 5);
    assert_eq!(result.orphan_nights, 3);
    assert_eq!(result.short_gaps, 2);
    assert_eq!(result.total_gap_nights, 9);
    assert_eq!(result.unbookable_gaps, 1);
    assert_eq!(result.suggested_min_nights, Some(1));
    assert!(result.potential_lost_revenue.is_none());
    let starts: Vec<&str> = result.gaps.iter().map(|g| g.start_date.as_str()).collect();
    assert_eq!(
        starts,
        vec![
            "2026-10-14",
            "2026-10-26",
            "2026-11-03",
            "2026-11-08",
            "2026-11-13"
        ]
    );
}

#[test]
fn live_calendar_revenue_uses_neighborhood_adr_and_measured_occupancy() {
    let neighborhood = NeighborhoodStats {
        location: "Lyon".into(),
        total_listings: 20,
        average_price: Some(120.0),
        median_price: Some(110.0),
        price_range: Some((60.0, 300.0)),
        average_rating: Some(4.7),
        property_type_distribution: vec![],
        superhost_percentage: None,
        currency: Some("$".into()),
        priced_listings: 18,
    };
    let est = compute_revenue_estimate(
        Some("12345678"),
        "Lyon",
        None,
        Some(&live_calendar()),
        Some(&neighborhood),
    )
    .expect("estimate");
    assert_eq!(est.adr_source, DataSource::NeighborhoodAverage);
    assert_eq!(est.occupancy_source, DataSource::Calendar);
    assert_eq!(est.occupancy_nights_measured, 64);
    let expected_monthly = 120.0 * (53.0 / 64.0) * 30.44;
    assert!((est.projected_monthly_revenue - expected_monthly).abs() < 1e-6);
    let september = &est.monthly_breakdown[0];
    assert_eq!(september.month, "2026-09");
    assert!((september.projected_revenue - 360.0).abs() < 1e-9);
}
