use scraper::{Html, Selector};

use crate::domain::calendar::{CalendarDay, PriceCalendar, UnavailabilityReason};
use crate::error::{AirbnbError, Result};

/// Parse price calendar from Airbnb listing page or calendar API response.
pub fn parse_price_calendar(html: &str, listing_id: &str) -> Result<PriceCalendar> {
    // Try __NEXT_DATA__ JSON first
    if let Some(calendar) = try_parse_next_data_calendar(html, listing_id) {
        return Ok(calendar);
    }

    // Try deferred state (current format with niobeClientData)
    if let Some(calendar) = try_parse_deferred_state_calendar(html, listing_id) {
        return Ok(calendar);
    }

    // Try parsing as raw JSON (for API responses)
    if let Some(calendar) = try_parse_json_response(html, listing_id) {
        return Ok(calendar);
    }

    Err(AirbnbError::Parse {
        reason: "could not extract calendar data from response".into(),
    })
}

fn try_parse_next_data_calendar(html: &str, listing_id: &str) -> Option<PriceCalendar> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r"script#__NEXT_DATA__").ok()?;
    let script = document.select(&selector).next()?;
    let json_text = script.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&json_text).ok()?;

    extract_calendar_from_json(&data, listing_id)
}

fn try_parse_deferred_state_calendar(html: &str, listing_id: &str) -> Option<PriceCalendar> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']").ok()?;

    for script in document.select(&selector) {
        let json_text = script.text().collect::<String>();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text) {
            // Try niobeClientData wrapper (current Airbnb format)
            if let Some(entries) = data.get("niobeClientData").and_then(|v| v.as_array()) {
                for entry in entries {
                    if let Some(inner) = entry.as_array().and_then(|arr| arr.get(1)) {
                        // Try PDP sections format for calendar metadata
                        if let Some(calendar) =
                            extract_calendar_from_pdp_sections(inner, listing_id)
                        {
                            return Some(calendar);
                        }
                        // Try legacy format
                        if let Some(calendar) = extract_calendar_from_json(inner, listing_id) {
                            return Some(calendar);
                        }
                    }
                }
            }
            // Legacy: try direct JSON structure
            if let Some(calendar) = extract_calendar_from_json(&data, listing_id) {
                return Some(calendar);
            }
        }
    }
    None
}

/// Extract calendar info from current Airbnb PDP sections format.
/// Note: The current Airbnb format loads calendar data dynamically via JavaScript,
/// so day-by-day availability is not available in the initial HTML response.
/// We extract what metadata is available from the `AVAILABILITY_CALENDAR_DEFAULT` section
/// and the `BOOK_IT_SIDEBAR` section.
fn extract_calendar_from_pdp_sections(
    data: &serde_json::Value,
    listing_id: &str,
) -> Option<PriceCalendar> {
    let pdp = data
        .get("data")?
        .get("presentation")?
        .get("stayProductDetailPage")?;
    let sections_container = pdp.get("sections")?;
    let sections = sections_container.get("sections")?.as_array()?;

    // Find BOOK_IT_SIDEBAR or BOOK_IT_CALENDAR_SHEET which may have pricing info
    let book_section = sections.iter().find_map(|s| {
        let stype = s
            .get("sectionComponentType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if stype == "BOOK_IT_SIDEBAR" || stype == "BOOK_IT_CALENDAR_SHEET" {
            s.get("section")
        } else {
            None
        }
    });

    // Find calendar section for metadata
    let calendar_section = sections.iter().find_map(|s| {
        if s.get("sectionComponentType").and_then(|v| v.as_str())
            == Some("AVAILABILITY_CALENDAR_DEFAULT")
        {
            s.get("section")
        } else {
            None
        }
    });

    // If we have booking section with price description items, extract them
    if let Some(book) = book_section
        && let Some(items) = book.get("descriptionItems").and_then(|v| v.as_array())
    {
        let mut days = Vec::new();
        for item in items {
            if let Some(day) = extract_calendar_day_from_booking_item(item) {
                days.push(day);
            }
        }
        if !days.is_empty() {
            let mut cal = PriceCalendar {
                listing_id: listing_id.to_string(),
                currency: "$".to_string(),
                days,
                average_price: None,
                occupancy_rate: None,
                min_price: None,
                max_price: None,
            };
            cal.compute_stats();
            return Some(cal);
        }
    }

    // Calendar data is loaded dynamically — not available in initial page HTML.
    // Return None to let the caller know we couldn't extract day-by-day data.
    // The calendar_section has metadata (title, maxGuestCapacity) but no day data.
    let _ = calendar_section;
    None
}

fn extract_calendar_day_from_booking_item(item: &serde_json::Value) -> Option<CalendarDay> {
    let title = item.get("title")?.as_str()?;
    // Check if this looks like a date/price item
    if title.contains("night") || title.contains("price") {
        return None; // This is a label, not a calendar day
    }
    // This is metadata like "Private room" or "1 bed" — not a calendar day
    None
}

fn try_parse_json_response(text: &str, listing_id: &str) -> Option<PriceCalendar> {
    let data: serde_json::Value = serde_json::from_str(text).ok()?;
    extract_calendar_from_json(&data, listing_id)
}

fn extract_calendar_from_json(data: &serde_json::Value, listing_id: &str) -> Option<PriceCalendar> {
    let calendar_data = find_calendar_data(data)?;
    let mut days = Vec::new();

    // Calendar data might be organized by months (camelCase or snake_case)
    if let Some(months) = calendar_data
        .get("calendarMonths")
        .or_else(|| calendar_data.get("calendar_months"))
        .and_then(|v| v.as_array())
    {
        for month in months {
            if let Some(month_days) = month.get("days").and_then(|v| v.as_array()) {
                for day in month_days {
                    if let Some(calendar_day) = extract_calendar_day(day) {
                        days.push(calendar_day);
                    }
                }
            }
        }
    }

    // Or it might be a flat array of days
    if days.is_empty()
        && let Some(arr) = calendar_data.as_array()
    {
        for day in arr {
            if let Some(calendar_day) = extract_calendar_day(day) {
                days.push(calendar_day);
            }
        }
    }

    // Or nested under "days" key
    if days.is_empty()
        && let Some(arr) = calendar_data.get("days").and_then(|v| v.as_array())
    {
        for day in arr {
            if let Some(calendar_day) = extract_calendar_day(day) {
                days.push(calendar_day);
            }
        }
    }

    if days.is_empty() {
        return None;
    }

    let currency = calendar_data
        .get("currency")
        .or_else(|| calendar_data.get("priceCurrency"))
        .and_then(|v| v.as_str())
        .unwrap_or("$")
        .to_string();

    let mut cal = PriceCalendar {
        listing_id: listing_id.to_string(),
        currency,
        days,
        average_price: None,
        occupancy_rate: None,
        min_price: None,
        max_price: None,
    };
    cal.compute_stats();
    Some(cal)
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

fn deep_find_calendar(data: &serde_json::Value, max_depth: u32) -> Option<&serde_json::Value> {
    if max_depth == 0 {
        return None;
    }
    match data {
        serde_json::Value::Object(map) => {
            // Look for "calendarMonths" / "calendar_months" keys
            if map.contains_key("calendarMonths") || map.contains_key("calendar_months") {
                return Some(data);
            }
            if let Some(days) = map.get("days")
                && days.is_array()
            {
                let arr = days.as_array().unwrap();
                let has_calendar_data = arr
                    .iter()
                    .any(|item| item.get("date").is_some() || item.get("calendarDate").is_some());
                if has_calendar_data {
                    return Some(data);
                }
            }
            for value in map.values() {
                if let Some(result) = deep_find_calendar(value, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            // Check if this array contains day-like objects
            let has_day_data = arr
                .iter()
                .any(|item| item.get("date").is_some() || item.get("calendarDate").is_some());
            if has_day_data && !arr.is_empty() {
                return Some(data);
            }
            for item in arr {
                if let Some(result) = deep_find_calendar(item, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

/// Parse a price string like "$150", "€120.50", "120" into f64.
fn parse_price_string(s: &str) -> Option<f64> {
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse::<f64>().ok()
}

/// Infer why a day is unavailable from JSON fields and the date itself.
fn infer_unavailability_reason(data: &serde_json::Value, date: &str) -> UnavailabilityReason {
    // Check if the date is in the past
    if let Ok(parsed) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        && parsed < chrono::Utc::now().date_naive()
    {
        return UnavailabilityReason::PastDate;
    }

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
    let date = data
        .get("date")
        .or_else(|| data.get("calendarDate"))
        .and_then(|v| v.as_str())?
        .to_string();

    let available = data
        .get("available")
        .or_else(|| data.get("isAvailable"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

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
                // String format: {"price": "$150"}
                .or_else(|| p.as_str().and_then(parse_price_string))
        })
        // v3 fallback: {"localPriceFormatted": "$95"}
        .or_else(|| {
            data.get("localPriceFormatted")
                .and_then(|v| v.as_str())
                .and_then(parse_price_string)
        })
        // v2 fallback: {"price_string": "$120"}
        .or_else(|| {
            data.get("price_string")
                .and_then(|v| v.as_str())
                .and_then(parse_price_string)
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
        Some(infer_unavailability_reason(data, &date))
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
        assert_eq!(calendar.currency, "EUR");
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
    fn calendar_day_unavailable_default() {
        let data: serde_json::Value =
            serde_json::from_str(r#"{"date":"2025-10-01","price":100.0}"#).unwrap();
        let day = extract_calendar_day(&data).unwrap();
        assert!(!day.available);
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
    fn parse_price_string_helper() {
        assert_eq!(parse_price_string("$150"), Some(150.0));
        assert_eq!(parse_price_string("€120.50"), Some(120.50));
        assert_eq!(parse_price_string("120"), Some(120.0));
        assert_eq!(parse_price_string("¥15000"), Some(15000.0));
        assert_eq!(parse_price_string(""), None);
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
}
