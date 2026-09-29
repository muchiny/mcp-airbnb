use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use scraper::Html;
use serde_json::Value;

use crate::adapters::price::{normalize_currency, parse_price_amount};
use crate::adapters::scraper::deferred_state::{
    deferred_state_json, next_data_json, niobe_payloads,
};
use crate::domain::calendar::{CalendarDay, PriceCalendar, UnavailabilityReason};
use crate::error::{AirbnbError, Result};

/// Parse a price calendar from a listing page (HTML) or an API body (JSON).
pub fn parse_price_calendar(html: &str, listing_id: &str) -> Result<PriceCalendar> {
    // API bodies are JSON: skip the HTML parser entirely.
    if let Ok(data) = serde_json::from_str::<Value>(html) {
        return parse_calendar_json(&data, listing_id);
    }
    let document = Html::parse_document(html);
    if let Some(calendar) =
        next_data_json(&document).and_then(|data| extract_calendar_from_json(&data, listing_id))
    {
        return Ok(calendar);
    }
    let states = deferred_state_json(&document);
    states
        .iter()
        .flat_map(niobe_payloads)
        .chain(states.iter())
        .find_map(|data| extract_calendar_from_json(data, listing_id))
        .ok_or_else(no_calendar_error)
}

/// Parse a calendar from decoded JSON: the GraphQL `PdpAvailabilityCalendar`
/// response or a legacy v2 body.
pub fn parse_calendar_json(data: &Value, listing_id: &str) -> Result<PriceCalendar> {
    extract_calendar_from_json(data, listing_id).ok_or_else(no_calendar_error)
}

fn no_calendar_error() -> AirbnbError {
    AirbnbError::UpstreamSchema {
        operation: "PdpAvailabilityCalendar".into(),
        detail: "no calendar days with an ISO date and an explicit availability flag".into(),
    }
}

fn extract_calendar_from_json(data: &Value, listing_id: &str) -> Option<PriceCalendar> {
    let calendar_data = find_calendar_data(data)?;
    // Keyed by date: sorted, one entry per date. A date listed by two months
    // (week-padded grids) keeps the entry of the month it belongs to.
    let mut by_date: BTreeMap<NaiveDate, (CalendarDay, bool)> = BTreeMap::new();

    if let Some(months) = calendar_data
        .get("calendarMonths")
        .or_else(|| calendar_data.get("calendar_months"))
        .and_then(Value::as_array)
    {
        for month in months {
            let owner = month_of(month);
            for day in month
                .get("days")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                insert_day(&mut by_date, day, owner);
            }
        }
    }
    if by_date.is_empty() {
        let flat = calendar_data
            .as_array()
            .or_else(|| calendar_data.get("days").and_then(Value::as_array));
        for day in flat.into_iter().flatten() {
            insert_day(&mut by_date, day, None);
        }
    }
    if by_date.is_empty() {
        return None;
    }

    let currency = calendar_data
        .get("currency")
        .or_else(|| calendar_data.get("priceCurrency"))
        .and_then(Value::as_str)
        .map(normalize_currency)
        .unwrap_or_default();

    let mut cal = PriceCalendar {
        listing_id: listing_id.to_string(),
        currency,
        days: by_date.into_values().map(|(day, _)| day).collect(),
        average_price: None,
        occupancy_rate: None,
        min_price: None,
        max_price: None,
    };
    cal.compute_stats();
    Some(cal)
}

/// `(year, month)` a `calendarMonths` entry covers, when it says so.
fn month_of(month: &Value) -> Option<(i32, u32)> {
    let year = i32::try_from(month.get("year")?.as_i64()?).ok()?;
    let number = u32::try_from(month.get("month")?.as_u64()?).ok()?;
    Some((year, number))
}

fn insert_day(
    by_date: &mut BTreeMap<NaiveDate, (CalendarDay, bool)>,
    raw: &Value,
    owner: Option<(i32, u32)>,
) {
    let Some(day) = extract_calendar_day(raw) else {
        return;
    };
    let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") else {
        return;
    };
    let in_own_month =
        owner.is_some_and(|(year, month)| date.year() == year && date.month() == month);
    let replace = by_date
        .get(&date)
        .is_none_or(|(_, existing_in_own_month)| in_own_month && !*existing_in_own_month);
    if replace {
        by_date.insert(date, (day, in_own_month));
    }
}

fn find_calendar_data(data: &serde_json::Value) -> Option<&serde_json::Value> {
    // If root object already has calendarMonths (camelCase or snake_case), return it directly
    if data.get("calendarMonths").is_some() || data.get("calendar_months").is_some() {
        return Some(data);
    }

    let paths: &[&[&str]] = &[
        &["props", "pageProps", "calendarData"],
        &["props", "pageProps", "listing", "calendarData"],
        &["data", "merlin", "pdpAvailabilityCalendar"],
    ];

    for path in paths {
        let mut current = data;
        let mut found = true;
        for key in *path {
            if let Some(next) = current.get(key) {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found {
            return Some(current);
        }
    }

    // Deep search for calendar-like data
    deep_find_calendar(data, 20)
}

fn deep_find_calendar(data: &Value, max_depth: u32) -> Option<&Value> {
    if max_depth == 0 {
        return None;
    }
    match data {
        Value::Object(map) => {
            if map.contains_key("calendarMonths") || map.contains_key("calendar_months") {
                return Some(data);
            }
            if map
                .get("days")
                .and_then(Value::as_array)
                .is_some_and(|days| is_day_list(days))
            {
                return Some(data);
            }
            map.values()
                .find_map(|value| deep_find_calendar(value, max_depth - 1))
        }
        Value::Array(items) => {
            if is_day_list(items) {
                return Some(data);
            }
            items
                .iter()
                .find_map(|item| deep_find_calendar(item, max_depth - 1))
        }
        _ => None,
    }
}

/// A list is a calendar only when every entry has an ISO date and an explicit
/// availability flag (a review list with `date` keys is not).
fn is_day_list(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(is_calendar_day)
}

fn is_calendar_day(item: &Value) -> bool {
    let has_iso_date = item
        .get("date")
        .or_else(|| item.get("calendarDate"))
        .and_then(Value::as_str)
        .is_some_and(|date| NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").is_ok());
    let has_flag = item
        .get("available")
        .or_else(|| item.get("isAvailable"))
        .is_some_and(Value::is_boolean);
    has_iso_date && has_flag
}

/// Infer why a day is unavailable from its JSON fields.
///
/// Past-date labelling is not done here. Adapters call
/// `PriceCalendar::classify_past_days` with one explicit reference date, so
/// parsing stays a pure function of its input.
fn infer_unavailability_reason(data: &serde_json::Value) -> UnavailabilityReason {
    // Check for booking status indicators (various Airbnb JSON formats)
    if let Some(status) = data
        .get("bookingStatusType")
        .or_else(|| data.get("booking_status_type"))
        .or_else(|| data.get("bookingStatus"))
        .and_then(|v| v.as_str())
    {
        let status_lower = status.to_lowercase();
        if status_lower.contains("booked") || status_lower.contains("reservation") {
            return UnavailabilityReason::Booked;
        }
    }

    // Check for host-blocked indicators
    if let Some(false) = data
        .get("autoAvailability")
        .or_else(|| data.get("auto_availability"))
        .and_then(serde_json::Value::as_bool)
    {
        return UnavailabilityReason::BlockedByHost;
    }

    // Check if blocked by host via a "blocked" or "hostBlocked" field
    if let Some(true) = data
        .get("hostBlocked")
        .or_else(|| data.get("host_blocked"))
        .or_else(|| data.get("blocked"))
        .and_then(serde_json::Value::as_bool)
    {
        return UnavailabilityReason::BlockedByHost;
    }

    // Check for minimum night restriction signals
    if let Some(true) = data
        .get("closedToArrival")
        .and_then(serde_json::Value::as_bool)
        && let Some(true) = data
            .get("closedToDeparture")
            .and_then(serde_json::Value::as_bool)
    {
        return UnavailabilityReason::MinNightRestriction;
    }

    UnavailabilityReason::Unknown
}

#[allow(clippy::cast_possible_truncation)]
fn extract_calendar_day(data: &serde_json::Value) -> Option<CalendarDay> {
    let raw_date = data
        .get("date")
        .or_else(|| data.get("calendarDate"))
        .and_then(Value::as_str)?;
    // Only ISO dates: analytics compare and slice them as YYYY-MM-DD, and a
    // byte slice of a non-ASCII string would abort the process.
    let date = NaiveDate::parse_from_str(raw_date.trim(), "%Y-%m-%d")
        .ok()?
        .format("%Y-%m-%d")
        .to_string();

    // A day without an explicit availability flag is not a calendar day
    // (for example a review with a `date` key).
    let available = data
        .get("available")
        .or_else(|| data.get("isAvailable"))
        .and_then(Value::as_bool)?;

    let price = data
        .get("price")
        .and_then(|p| {
            // Direct number: {"price": 120.0}
            p.as_f64()
                // Nested amount (v3): {"price": {"amount": 120.0}}
                .or_else(|| p.get("amount").and_then(serde_json::Value::as_f64))
                // v2 format: {"price": {"local_price": 120.0}}
                .or_else(|| p.get("local_price").and_then(serde_json::Value::as_f64))
                // v2 format: {"price": {"native_price": 120.0}}
                .or_else(|| p.get("native_price").and_then(serde_json::Value::as_f64))
                // Merlin calendar (2026): {"price": {"localPriceFormatted": "$95"}}, null when hidden
                .or_else(|| {
                    p.get("localPriceFormatted")
                        .and_then(serde_json::Value::as_str)
                        .and_then(parse_price_amount)
                })
                // String format: {"price": "$150"}
                .or_else(|| p.as_str().and_then(parse_price_amount))
        })
        // v3 fallback: {"localPriceFormatted": "$95"}
        .or_else(|| {
            data.get("localPriceFormatted")
                .and_then(|v| v.as_str())
                .and_then(parse_price_amount)
        })
        // v2 fallback: {"price_string": "$120"}
        .or_else(|| {
            data.get("price_string")
                .and_then(|v| v.as_str())
                .and_then(parse_price_amount)
        });

    let min_nights = data
        .get("minNights")
        .or_else(|| data.get("minimumNights"))
        .or_else(|| data.get("min_nights"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let max_nights = data
        .get("maxNights")
        .or_else(|| data.get("maximumNights"))
        .or_else(|| data.get("max_nights"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let closed_to_arrival = data
        .get("closedToArrival")
        .and_then(serde_json::Value::as_bool);

    let closed_to_departure = data
        .get("closedToDeparture")
        .and_then(serde_json::Value::as_bool);

    // Infer unavailability reason for unavailable days
    let unavailability_reason = if available {
        None
    } else {
        Some(infer_unavailability_reason(data))
    };

    Some(CalendarDay {
        date,
        price,
        available,
        min_nights,
        max_nights,
        closed_to_arrival,
        closed_to_departure,
        unavailability_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_calendar_from_json() {
        let json = r#"{"calendarMonths":[{"days":[
            {"date":"2025-06-01","available":true,"price":{"amount":150.0},"minNights":2},
            {"date":"2025-06-02","available":false,"price":{"amount":150.0},"minNights":2}
        ]}],"currency":"USD"}"#;

        let calendar = parse_price_calendar(json, "123").unwrap();
        assert_eq!(calendar.days.len(), 2);
        assert!(calendar.days[0].available);
        assert!(!calendar.days[1].available);
        assert_eq!(calendar.days[0].price, Some(150.0));
    }

    #[test]
    fn parse_empty_html_returns_error() {
        let result = parse_price_calendar("<html><body></body></html>", "123");
        assert!(result.is_err());
    }

    #[test]
    fn parse_calendar_flat_array() {
        let json = r#"{"wrapper":{"data":[
            {"date":"2025-07-01","available":true,"price":100.0},
            {"date":"2025-07-02","available":true,"price":110.0},
            {"date":"2025-07-03","available":false,"price":120.0},
            {"date":"2025-07-04","available":true,"price":130.0},
            {"date":"2025-07-05","available":true,"price":140.0},
            {"date":"2025-07-06","available":true,"price":150.0}
        ]}}"#;

        let calendar = parse_price_calendar(json, "1").unwrap();
        assert_eq!(calendar.days.len(), 6);
        assert!(calendar.days[0].available);
        assert!(!calendar.days[2].available);
    }

    #[test]
    fn parse_calendar_nested_days_key() {
        let json = r#"{"wrapper":{"inner":{"days":[
            {"date":"2025-08-01","available":true,"price":{"amount":200.0}},
            {"date":"2025-08-02","available":false,"price":{"amount":210.0}},
            {"date":"2025-08-03","available":true,"price":{"amount":220.0}},
            {"date":"2025-08-04","available":true,"price":{"amount":230.0}},
            {"date":"2025-08-05","available":true,"price":{"amount":240.0}},
            {"date":"2025-08-06","available":true,"price":{"amount":250.0}}
        ]}}}"#;

        let calendar = parse_price_calendar(json, "2").unwrap();
        assert_eq!(calendar.days.len(), 6);
        assert_eq!(calendar.days[0].price, Some(200.0));
    }

    #[test]
    fn parse_calendar_deferred_state() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"calendarMonths":[{"days":[
            {"date":"2025-09-01","available":true,"price":{"amount":300.0},"minNights":3}
        ]}],"currency":"EUR"}
        </script></head><body></body></html>"#;

        let calendar = parse_price_calendar(html, "3").unwrap();
        assert_eq!(calendar.days.len(), 1);
        assert_eq!(calendar.currency, "\u{20ac}");
        assert_eq!(calendar.days[0].min_nights, Some(3));
    }

    #[test]
    fn calendar_day_price_from_string() {
        let data: serde_json::Value =
            serde_json::from_str(r#"{"date":"2025-10-01","available":true,"price":"$150"}"#)
                .unwrap();
        let day = extract_calendar_day(&data).unwrap();
        assert_eq!(day.price, Some(150.0));
    }

    #[test]
    fn calendar_day_without_availability_flag_is_rejected() {
        let data: serde_json::Value =
            serde_json::from_str(r#"{"date":"2025-10-01","price":100.0}"#).unwrap();
        assert!(extract_calendar_day(&data).is_none());
    }

    #[test]
    fn parse_calendar_short_array() {
        let json = r#"{"wrapper":{"data":[
            {"date":"2025-07-01","available":true,"price":100.0},
            {"date":"2025-07-02","available":true,"price":110.0}
        ]}}"#;

        let calendar = parse_price_calendar(json, "1").unwrap();
        assert_eq!(calendar.days.len(), 2);
    }

    #[test]
    fn deep_find_calendar_respects_max_depth() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"a":{"b":{"c":{"calendarMonths":[{"days":[{"date":"2025-01-01","available":true}]}]}}}}"#
        ).unwrap();
        let shallow = deep_find_calendar(&data, 1);
        assert!(shallow.is_none());

        let deep = deep_find_calendar(&data, 20);
        assert!(deep.is_some());
    }

    #[test]
    fn parse_graphql_api_response() {
        // Simulates the actual GraphQL PdpAvailabilityCalendar response format
        let json = r#"{
            "data": {
                "merlin": {
                    "pdpAvailabilityCalendar": {
                        "calendarMonths": [
                            {
                                "month": 3,
                                "year": 2026,
                                "days": [
                                    {"calendarDate": "2026-03-01", "available": true, "price": {"amount": 120.0}, "minNights": 2},
                                    {"calendarDate": "2026-03-02", "available": true, "price": {"amount": 120.0}, "minNights": 2},
                                    {"calendarDate": "2026-03-03", "available": false, "price": {"amount": 130.0}, "minNights": 2},
                                    {"calendarDate": "2026-03-04", "available": true, "price": {"amount": 125.0}, "minNights": 3}
                                ]
                            },
                            {
                                "month": 4,
                                "year": 2026,
                                "days": [
                                    {"calendarDate": "2026-04-01", "available": true, "price": {"amount": 140.0}, "minNights": 2},
                                    {"calendarDate": "2026-04-02", "available": true, "price": {"amount": 140.0}, "minNights": 2}
                                ]
                            }
                        ],
                        "currency": "USD"
                    }
                }
            }
        }"#;

        let calendar = parse_price_calendar(json, "12345").unwrap();
        assert_eq!(calendar.listing_id, "12345");
        assert_eq!(calendar.days.len(), 6);
        assert_eq!(calendar.days[0].date, "2026-03-01");
        assert!(calendar.days[0].available);
        assert_eq!(calendar.days[0].price, Some(120.0));
        assert_eq!(calendar.days[0].min_nights, Some(2));
        assert!(!calendar.days[2].available);
        assert_eq!(calendar.days[4].date, "2026-04-01");
        assert_eq!(calendar.days[4].price, Some(140.0));
    }

    #[test]
    fn parse_graphql_response_with_local_price() {
        let json = r#"{
            "data": {
                "merlin": {
                    "pdpAvailabilityCalendar": {
                        "calendarMonths": [{
                            "days": [
                                {"date": "2026-05-01", "isAvailable": true, "localPriceFormatted": "$95", "minimumNights": 1}
                            ]
                        }],
                        "priceCurrency": "EUR"
                    }
                }
            }
        }"#;

        let calendar = parse_price_calendar(json, "999").unwrap();
        assert_eq!(calendar.days.len(), 1);
        assert!(calendar.days[0].available);
        assert_eq!(calendar.days[0].price, Some(95.0));
        assert_eq!(calendar.days[0].min_nights, Some(1));
    }

    #[test]
    fn parse_v2_calendar_with_local_price() {
        // v2 REST API format with calendar_months (snake_case) and price.local_price
        let json = r#"{
            "calendar_months": [
                {
                    "month": 3,
                    "year": 2026,
                    "days": [
                        {
                            "date": "2026-03-01",
                            "available": true,
                            "price": {"local_price": 120.0, "native_price": 120.0},
                            "price_string": "$120",
                            "min_nights": 2,
                            "max_nights": 30
                        },
                        {
                            "date": "2026-03-02",
                            "available": false,
                            "price": {"local_price": 130.0, "native_price": 130.0},
                            "price_string": "$130",
                            "min_nights": 2,
                            "max_nights": 30
                        }
                    ]
                }
            ]
        }"#;

        let calendar = parse_price_calendar(json, "v2test").unwrap();
        assert_eq!(calendar.listing_id, "v2test");
        assert_eq!(calendar.days.len(), 2);
        assert!(calendar.days[0].available);
        assert_eq!(calendar.days[0].price, Some(120.0));
        assert!(!calendar.days[1].available);
        assert_eq!(calendar.days[1].price, Some(130.0));
        assert_eq!(calendar.days[0].min_nights, Some(2));
        assert_eq!(calendar.days[0].max_nights, Some(30));
    }

    #[test]
    fn parse_v2_calendar_with_native_price() {
        let json = r#"{
            "calendar_months": [{
                "days": [
                    {"date": "2026-06-15", "available": true, "price": {"native_price": 85.5}, "min_nights": 1}
                ]
            }]
        }"#;

        let calendar = parse_price_calendar(json, "np").unwrap();
        assert_eq!(calendar.days[0].price, Some(85.5));
    }

    #[test]
    fn parse_v2_calendar_price_string_fallback() {
        // When price object has no numeric fields, fall back to price_string
        let data: serde_json::Value = serde_json::from_str(
            r#"{"date": "2026-07-01", "available": true, "price_string": "€95"}"#,
        )
        .unwrap();
        let day = extract_calendar_day(&data).unwrap();
        assert_eq!(day.price, Some(95.0));
    }

    #[test]
    fn v2_full_response_with_conditions() {
        // Simulates the actual v2 REST API response with _format=with_conditions
        let json = r#"{
            "calendar_months": [
                {
                    "month": 2,
                    "year": 2026,
                    "days": [
                        {
                            "date": "2026-02-01",
                            "available": true,
                            "price": {"local_price": 200.0, "native_price": 200.0, "local_price_formatted": "$200", "native_currency": "USD"},
                            "price_string": "$200",
                            "min_nights": 3,
                            "max_nights": 14
                        },
                        {
                            "date": "2026-02-02",
                            "available": true,
                            "price": {"local_price": 220.0, "native_price": 220.0},
                            "price_string": "$220",
                            "min_nights": 3,
                            "max_nights": 14
                        },
                        {
                            "date": "2026-02-03",
                            "available": false,
                            "price": {"local_price": 0, "native_price": 0},
                            "price_string": "$0",
                            "min_nights": 3,
                            "max_nights": 14
                        }
                    ]
                }
            ]
        }"#;

        let calendar = parse_price_calendar(json, "full-v2").unwrap();
        assert_eq!(calendar.days.len(), 3);
        assert_eq!(calendar.days[0].price, Some(200.0));
        assert_eq!(calendar.days[1].price, Some(220.0));
        // Price 0 means unavailable day's price — still parsed as 0.0
        assert_eq!(calendar.days[2].price, Some(0.0));
        assert!(!calendar.days[2].available);
        assert_eq!(calendar.days[0].min_nights, Some(3));
        assert_eq!(calendar.days[0].max_nights, Some(14));
        // Stats should be computed
        assert!(calendar.average_price.is_some());
    }

    #[test]
    fn merlin_nested_local_price_formatted_is_read() {
        let json = r#"{"data":{"merlin":{"pdpAvailabilityCalendar":{"calendarMonths":[{"month":10,"year":2030,"days":[
            {"calendarDate":"2030-10-01","available":true,"minNights":2,"price":{"__typename":"MerlinCalendarDayPrice","localPriceFormatted":"$95"}},
            {"calendarDate":"2030-10-02","available":true,"minNights":2,"price":{"__typename":"MerlinCalendarDayPrice","localPriceFormatted":null}}
        ]}]}}}}"#;
        let cal = parse_price_calendar(json, "1").unwrap();
        assert_eq!(cal.days[0].price, Some(95.0));
        assert_eq!(cal.days[1].price, None);
    }

    #[test]
    fn web_capture_2026_09_has_no_prices() {
        let json =
            crate::test_helpers::fixture_json("graphql/PdpAvailabilityCalendar.response.json");
        let cal = parse_price_calendar(&json.to_string(), "38817969").unwrap();
        assert_eq!(cal.days.len(), 365);
        assert!(cal.days.iter().all(|d| d.price.is_none()));
        assert!(cal.days.iter().all(|d| d.min_nights.is_some()));
        assert!(cal.average_price.is_none());
    }

    #[test]
    fn calendar_without_currency_is_unknown_not_dollar() {
        let json = r#"{"calendarMonths":[{"days":[{"date":"2026-06-01","available":true}]}]}"#;
        assert_eq!(parse_price_calendar(json, "1").unwrap().currency, "");
    }

    #[test]
    fn real_calendar_fixture_is_sorted_unique_and_iso_dated() {
        let json = crate::test_helpers::fixture_json("p1b/calendar.json");
        let calendar = parse_calendar_json(&json, "38817969").unwrap();
        let dates: Vec<&str> = calendar.days.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(
            dates,
            [
                "2026-09-28",
                "2026-09-29",
                "2026-09-30",
                "2026-10-13",
                "2026-10-14",
                "2026-10-15"
            ]
        );
        assert!(calendar.days[4].available);
        assert_eq!(calendar.contiguous_runs().len(), 2);
    }

    #[test]
    fn padded_month_grids_are_deduplicated_in_favour_of_the_owning_month() {
        // Legacy v2 grids repeat the neighbouring months' days as padding.
        let json = serde_json::json!({"calendar_months": [
            {"month": 10, "year": 2026, "days": [
                {"date": "2026-10-31", "available": false},
                {"date": "2026-11-01", "available": true}
            ]},
            {"month": 11, "year": 2026, "days": [
                {"date": "2026-10-31", "available": true},
                {"date": "2026-11-01", "available": false},
                {"date": "2026-11-02", "available": false}
            ]}
        ]});
        let calendar = parse_calendar_json(&json, "1").unwrap();
        let days: Vec<(&str, bool)> = calendar
            .days
            .iter()
            .map(|d| (d.date.as_str(), d.available))
            .collect();
        assert_eq!(
            days,
            [
                ("2026-10-31", false),
                ("2026-11-01", false),
                ("2026-11-02", false)
            ]
        );
    }

    #[test]
    fn non_iso_dates_are_dropped_before_analytics_can_slice_them() {
        let json = serde_json::json!({"data": {"merlin": {"pdpAvailabilityCalendar": {"calendarMonths": [{"days": [
            {"calendarDate": "2026-0\u{e9}-01", "available": true},
            {"calendarDate": "\u{438}\u{44e}\u{43d}\u{44c} 2026", "available": true},
            {"calendarDate": "2026-06-02", "available": true}
        ]}]}}}});
        let calendar = parse_calendar_json(&json, "1").unwrap();
        assert_eq!(calendar.days.len(), 1);
        assert_eq!(calendar.days[0].date, "2026-06-02");
    }

    #[test]
    fn review_arrays_with_a_date_key_are_not_a_calendar() {
        let reviews = serde_json::json!({"data": {"presentation": {"x": {"reviews": [
            {"date": "2025-03-01", "comments": "Great"},
            {"date": "2025-02-11", "comments": "Nice"}
        ]}}}});
        let html =
            crate::test_helpers::niobe_page("niobeClientData", &[("StaysPdpReviews:{}", &reviews)]);
        let err = parse_price_calendar(&html, "1").unwrap_err();
        assert!(matches!(err, AirbnbError::UpstreamSchema { .. }), "{err}");
    }

    #[test]
    fn listing_page_without_calendar_data_is_an_error_not_an_empty_calendar() {
        let pdp = crate::test_helpers::fixture_json("p1b/pdp_apartment.json");
        let html = crate::test_helpers::niobe_page(
            "niobeMinimalClientData",
            &[("StaysPdpSections:{}", &pdp)],
        );
        assert!(parse_price_calendar(&html, "38817969").is_err());
    }

    #[test]
    fn parser_leaves_past_labelling_to_the_caller() {
        let json = r#"{"calendarMonths":[{"days":[
            {"date":"2020-01-01","available":false,"minNights":1}
        ]}],"currency":"USD"}"#;
        let calendar = parse_price_calendar(json, "1").unwrap();
        assert_eq!(
            calendar.days[0].unavailability_reason,
            Some(UnavailabilityReason::Unknown)
        );
    }
}
