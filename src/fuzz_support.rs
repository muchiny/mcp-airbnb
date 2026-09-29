//! Panic-freedom harness shared by the cargo-fuzz targets in `fuzz/` and by
//! the stable seed-replay test `tests/fuzz_seed_replay_test.rs`.
//!
//! Each function takes raw fuzzer bytes, must never panic, and returns
//! `true` when the input got past the parser's structural gate, so the
//! replay test can prove that the committed seeds reach deep code. The
//! bodies live here, not in `fuzz/fuzz_targets/*.rs`, so that a parser
//! signature change breaks `cargo build` on stable instead of silently
//! breaking the nightly-only fuzz crate.

use crate::adapters::graphql::parsers::{detail, host, review, search};
use crate::adapters::scraper::{calendar_parser, detail_parser, review_parser, search_parser};
use crate::adapters::shared::extract_api_key;
use crate::domain::analytics::{
    compute_gap_finder, compute_occupancy_estimate, compute_price_trends,
};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing_id::validate_listing_id;
use crate::domain::search_params::SearchParams;

/// Listing id handed to parsers that need one.
const LISTING_ID: &str = "12345";
/// Base URL handed to parsers that build listing links.
const BASE_URL: &str = "https://www.airbnb.com";
/// Fixed "today" (year, month, day) for date-dependent code, so a crash
/// input found by the fuzzer reproduces on any later day.
const FUZZ_TODAY: (i32, u32, u32) = (2026, 9, 28);

fn text(data: &[u8]) -> Option<&str> {
    std::str::from_utf8(data).ok()
}

fn json(data: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(data).ok()
}

fn fuzz_today() -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(FUZZ_TODAY.0, FUZZ_TODAY.1, FUZZ_TODAY.2)
}

/// HTML search page → `search_parser::parse_search_results`.
pub fn scraper_search_html(data: &[u8]) -> bool {
    text(data).is_some_and(|html| search_parser::parse_search_results(html, BASE_URL).is_ok())
}

/// HTML listing page → `detail_parser::parse_listing_detail`.
pub fn scraper_detail_html(data: &[u8]) -> bool {
    text(data)
        .is_some_and(|html| detail_parser::parse_listing_detail(html, LISTING_ID, BASE_URL).is_ok())
}

/// HTML listing page → `detail_parser::parse_host_profile` (`MEET_YOUR_HOST`).
pub fn scraper_host_profile_html(data: &[u8]) -> bool {
    text(data).is_some_and(|html| detail_parser::parse_host_profile(html).is_ok())
}

/// HTML listing page → `review_parser::parse_reviews`.
pub fn scraper_reviews_html(data: &[u8]) -> bool {
    text(data).is_some_and(|html| review_parser::parse_reviews(html, LISTING_ID).is_ok())
}

/// HTML page or raw calendar JSON → `calendar_parser::parse_price_calendar`.
pub fn scraper_calendar(data: &[u8]) -> bool {
    text(data).is_some_and(|body| calendar_parser::parse_price_calendar(body, LISTING_ID).is_ok())
}

/// `StaysSearch` JSON → `search::parse_search_response` (first page).
pub fn graphql_search(data: &[u8]) -> bool {
    json(data).is_some_and(|value| search::parse_search_response(&value, BASE_URL).is_ok())
}

/// `StaysPdpSections` JSON → `detail::parse_detail_response`.
pub fn graphql_detail(data: &[u8]) -> bool {
    json(data)
        .is_some_and(|value| detail::parse_detail_response(&value, LISTING_ID, BASE_URL).is_ok())
}

/// `StaysPdpSections` JSON → `host::parse_host_response`.
pub fn graphql_host(data: &[u8]) -> bool {
    json(data).is_some_and(|value| host::parse_host_response(&value).is_ok())
}

/// `StaysPdpReviewsQuery` JSON → `review::parse_reviews_response` (offset 0).
pub fn graphql_reviews(data: &[u8]) -> bool {
    json(data).is_some_and(|value| review::parse_reviews_response(&value, LISTING_ID).is_ok())
}

/// Homepage HTML → `shared::extract_api_key`.
pub fn api_key(data: &[u8]) -> bool {
    text(data).and_then(extract_api_key).is_some()
}

/// Calendar response → `parse_price_calendar` → past-day classification →
/// occupancy, price-trend and gap analytics. PARSE-12 and MATH-17 could
/// abort the server on this path.
pub fn calendar_to_analytics(data: &[u8]) -> bool {
    let Some(body) = text(data) else {
        return false;
    };
    let Ok(calendar) = calendar_parser::parse_price_calendar(body, LISTING_ID) else {
        return false;
    };
    run_calendar_analytics(calendar);
    true
}

/// Any `PriceCalendar` JSON (arbitrary date strings, prices and flags) →
/// the same classification and analytics, bypassing the parser's own date
/// checks.
pub fn calendar_model_analytics(data: &[u8]) -> bool {
    let Ok(calendar) = serde_json::from_slice::<PriceCalendar>(data) else {
        return false;
    };
    run_calendar_analytics(calendar);
    true
}

/// Listing-id validation plus `SearchParams::validate_at` on a fixed day.
/// Input lines are location, checkin, checkout, adults.
pub fn input_validation(data: &[u8]) -> bool {
    let Some(input) = text(data) else {
        return false;
    };
    let id_ok = validate_listing_id(input).is_ok();
    let mut lines = input.split('\n');
    let params = SearchParams {
        location: lines.next().unwrap_or_default().to_string(),
        checkin: lines.next().map(str::to_string),
        checkout: lines.next().map(str::to_string),
        adults: lines.next().and_then(|n| n.trim().parse().ok()),
        ..SearchParams::default()
    };
    let params_ok = fuzz_today().is_some_and(|today| params.validate_at(today).is_ok());
    id_ok || params_ok
}

/// What the adapters do after parsing (P2: `classify_past_days`), then
/// what the tools compute and print.
fn run_calendar_analytics(mut calendar: PriceCalendar) {
    if let Some(today) = fuzz_today() {
        calendar.classify_past_days(today);
    }
    let occupancy = compute_occupancy_estimate(LISTING_ID, &calendar);
    let trends = compute_price_trends(LISTING_ID, &calendar);
    let gaps = compute_gap_finder(LISTING_ID, &calendar);
    // The MCP layer renders these with Display; formatting must not panic either.
    let _ = (occupancy.to_string(), trends.to_string(), gaps.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    type Harness = fn(&[u8]) -> bool;

    const HARNESSES: [(&str, Harness); 13] = [
        ("scraper_search_html", scraper_search_html),
        ("scraper_detail_html", scraper_detail_html),
        ("scraper_host_profile_html", scraper_host_profile_html),
        ("scraper_reviews_html", scraper_reviews_html),
        ("scraper_calendar", scraper_calendar),
        ("graphql_search", graphql_search),
        ("graphql_detail", graphql_detail),
        ("graphql_host", graphql_host),
        ("graphql_reviews", graphql_reviews),
        ("api_key", api_key),
        ("calendar_to_analytics", calendar_to_analytics),
        ("calendar_model_analytics", calendar_model_analytics),
        ("input_validation", input_validation),
    ];

    #[test]
    fn every_harness_rejects_non_utf8_bytes_without_panicking() {
        for (name, harness) in HARNESSES {
            assert!(
                !harness(&[0xff, 0xfe, 0x00, 0xc3]),
                "{name} accepted invalid UTF-8"
            );
        }
    }

    #[test]
    fn every_harness_survives_empty_and_tiny_inputs() {
        let inputs: [&[u8]; 6] = [
            b"",
            b"{",
            b"[]",
            b"null",
            b"<html>",
            b"\"api_config\":{\"key\":\"",
        ];
        for (_, harness) in HARNESSES {
            for input in inputs {
                let _ = harness(input);
            }
        }
    }

    #[test]
    fn api_key_handles_truncated_empty_and_multibyte_markers() {
        let page = b"<script>window.__config = {\"api_config\":{\"key\":\"abc123\"}}</script>";
        assert!(api_key(page));
        assert!(
            !api_key(b"<script>{\"api_config\":{\"key\":\""),
            "marker at the very end of the page"
        );
        assert!(!api_key(b"{\"api_config\":{\"key\":\"\"}}"), "empty key");
        assert!(
            api_key("{\"api_config\":{\"key\":\"clé-ü\"}}".as_bytes()),
            "multibyte key"
        );
        assert!(!api_key(&[0xff, 0xfe]), "invalid UTF-8");
    }

    #[test]
    fn calendar_model_survives_non_ascii_and_truncated_dates() {
        let calendar = r#"{"listing_id":"1","currency":"USD","days":[
            {"date":"２０２７-10-01","price":120.0,"available":false,"min_nights":1},
            {"date":"2027","price":null,"available":true,"min_nights":null},
            {"date":"é","price":80.0,"available":true,"min_nights":2},
            {"date":"","price":null,"available":false,"min_nights":null}]}"#;
        // A panic here aborts the release server (panic = "abort"): MATH-17, PARSE-12.
        // The JSON is a valid PriceCalendar, so the harness must reach the analytics.
        assert!(calendar_model_analytics(calendar.as_bytes()));
    }

    #[test]
    fn calendar_harnesses_run_analytics_on_valid_calendars() {
        let model = r#"{"listing_id":"1","currency":"USD","days":[
            {"date":"2027-10-01","price":120.0,"available":true,"min_nights":1},
            {"date":"2027-10-02","price":null,"available":false,"min_nights":1,"unavailability_reason":"Booked"},
            {"date":"2027-10-03","price":95.0,"available":true,"min_nights":2}]}"#;
        assert!(calendar_model_analytics(model.as_bytes()));
        let response = r#"{"data":{"merlin":{"pdpAvailabilityCalendar":{"calendarMonths":[{"month":10,"year":2027,"days":[
            {"calendarDate":"2027-10-01","available":true,"minNights":2,"maxNights":365,"price":{"localPriceFormatted":"$120"}},
            {"calendarDate":"2027-10-02","available":false,"minNights":2,"maxNights":365,"price":{"localPriceFormatted":null}}]}]}}}}"#;
        assert!(calendar_to_analytics(response.as_bytes()));
    }

    #[test]
    fn input_validation_accepts_a_listing_id_and_rejects_empty_input() {
        assert!(input_validation(b"12345"));
        assert!(!input_validation(b""));
    }

    #[test]
    fn input_validation_uses_a_fixed_today() {
        // Valid on 2026-09-28 (FUZZ_TODAY) whatever the real date is, so a
        // crash input found by the fuzzer reproduces on any later day.
        assert!(input_validation(b"Lyon, France\n2026-10-01\n2026-10-04\n2"));
        assert!(
            !input_validation(b"Lyon, France\n2026-09-01\n2026-09-04\n2"),
            "a check-in before FUZZ_TODAY is in the past (P4 I8)"
        );
    }
}
