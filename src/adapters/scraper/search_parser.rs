use std::collections::HashSet;

use scraper::{Html, Selector};

use crate::adapters::price::{normalize_currency, parse_price_amount};
use crate::adapters::scraper::deferred_state::{
    deferred_state_json, next_data_json, niobe_payloads,
};
use crate::adapters::stay_search::listing_from_stay_search_result;
use crate::domain::listing::{Listing, SearchResult};
use crate::error::{AirbnbError, Result};

/// Extract search results from Airbnb HTML.
///
/// The page is parsed into a DOM once. Tiers: `__NEXT_DATA__` JSON, the
/// `data-deferred-state` payloads (`niobeClientData` /
/// `niobeMinimalClientData`), then CSS selectors.
pub fn parse_search_results(html: &str, base_url: &str) -> Result<SearchResult> {
    let document = Html::parse_document(html);
    if let Some(result) =
        next_data_json(&document).and_then(|data| extract_listings_from_json(&data, base_url))
    {
        return Ok(result);
    }
    if let Some(result) = try_parse_deferred_state(&document, base_url) {
        return Ok(result);
    }
    parse_search_css(&document, base_url)
}

fn try_parse_deferred_state(document: &Html, base_url: &str) -> Option<SearchResult> {
    let states = deferred_state_json(document);
    // Query payloads first, then the raw state object (legacy layout).
    states
        .iter()
        .flat_map(niobe_payloads)
        .chain(states.iter())
        .find_map(|data| extract_listings_from_json(data, base_url))
}

fn extract_listings_from_json(data: &serde_json::Value, base_url: &str) -> Option<SearchResult> {
    // Navigate through various known JSON structures
    let mut listings = Vec::new();
    if let Some(sections) = find_search_sections(data) {
        listings.extend(
            sections
                .into_iter()
                .filter_map(|section| extract_listing_from_section(section, base_url)),
        );
    } else if let Some(sections) = deep_find_listings(data, 20) {
        // Heuristic tier: the first array holding an `id` could be anything
        // (photo ids, section ids, cursors). Legacy cards tolerate a missing
        // price, so here a card must be priced to count as a listing.
        listings.extend(
            sections
                .into_iter()
                .filter_map(|section| extract_listing_from_section(section, base_url))
                .filter(|listing| listing.known_price().is_some()),
        );
    }

    if listings.is_empty() {
        return None;
    }

    // Try to find pagination cursor
    let next_cursor = find_pagination_cursor(data);

    Some(SearchResult {
        total_count: None,
        listings,
        next_cursor,
    })
}

/// Known search paths only. The heuristic `deep_find_listings` tier is applied
/// by `extract_listings_from_json`, which filters its cards.
fn find_search_sections(data: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    // Airbnb structures data in various nested paths
    let paths: &[&[&str]] = &[
        &["props", "pageProps", "searchResults"],
        &[
            "data",
            "presentation",
            "staysSearch",
            "results",
            "searchResults",
        ],
    ];

    for path in paths {
        if let Some(sections) = navigate_json(data, path)
            && let Some(arr) = sections.as_array()
        {
            return Some(arr.iter().collect());
        }
    }

    None
}

fn navigate_json<'a>(data: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = data;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

fn deep_find_listings(data: &serde_json::Value, max_depth: u32) -> Option<Vec<&serde_json::Value>> {
    if max_depth == 0 {
        return None;
    }
    // Recursively search for arrays containing objects with listing IDs
    match data {
        serde_json::Value::Array(arr) => {
            let has_listings = arr.iter().any(|item| {
                item.get("listing").is_some()
                    || item.get("id").and_then(|v| v.as_str()).is_some()
                    || item.get("listingId").is_some()
            });
            if has_listings && !arr.is_empty() {
                return Some(arr.iter().collect());
            }
            // Search deeper
            for item in arr {
                if let Some(result) = deep_find_listings(item, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        serde_json::Value::Object(map) => {
            for value in map.values() {
                if let Some(result) = deep_find_listings(value, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_listing_from_section(section: &serde_json::Value, base_url: &str) -> Option<Listing> {
    // Current format (niobeClientData / `StaySearchResult`), shared with GraphQL.
    if section.get("demandStayListing").is_some() || section.get("structuredDisplayPrice").is_some()
    {
        return listing_from_stay_search_result(section, base_url);
    }

    // Legacy format: listing nested under "listing" key or flat
    extract_listing_legacy_format(section, base_url)
}

/// Extract listing from legacy Airbnb format (__`NEXT_DATA`__ style)
#[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
fn extract_listing_legacy_format(section: &serde_json::Value, base_url: &str) -> Option<Listing> {
    let listing_data = section.get("listing").unwrap_or(section);

    let id = listing_data
        .get("id")
        .or_else(|| listing_data.get("listingId"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })?;

    let name = listing_data
        .get("name")
        .or_else(|| listing_data.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown listing")
        .to_string();

    let location = listing_data
        .get("city")
        .or_else(|| listing_data.get("location"))
        .or_else(|| listing_data.get("publicAddress"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // A listing without a price is still a listing: 0.0 = unknown (see
    // `Listing::known_price`). Dropping it hid unpriced listings entirely.
    let price_per_night = extract_price_legacy(section)
        .or_else(|| extract_price_legacy(listing_data))
        .unwrap_or(0.0);

    let currency = section
        .get("pricingQuote")
        .and_then(|pq| {
            pq.get("price")
                .and_then(|p| p.get("currencySymbol").or_else(|| p.get("currency")))
                .or_else(|| pq.get("currencySymbol").or_else(|| pq.get("currency")))
        })
        .or_else(|| {
            listing_data
                .get("currency")
                .or_else(|| listing_data.get("priceCurrency"))
        })
        .and_then(|v| v.as_str())
        .map(normalize_currency)
        .unwrap_or_default();

    let rating = listing_data
        .get("avgRating")
        .or_else(|| {
            listing_data
                .get("avgRatingLocalized")
                .and_then(|_| listing_data.get("avgRating"))
        })
        .and_then(serde_json::Value::as_f64);

    let review_count = listing_data
        .get("reviewsCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;

    let thumbnail_url = listing_data
        .get("contextualPictures")
        .and_then(|pics| pics.as_array())
        .and_then(|arr| arr.first())
        .and_then(|pic| pic.get("picture"))
        .and_then(|p| p.as_str())
        .or_else(|| {
            listing_data
                .get("thumbnail")
                .or_else(|| listing_data.get("pictureUrl"))
                .and_then(|v| v.as_str())
        })
        .map(String::from);

    let property_type = listing_data
        .get("roomType")
        .or_else(|| listing_data.get("propertyType"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let host_name = listing_data
        .get("user")
        .and_then(|u| u.get("firstName"))
        .or_else(|| listing_data.get("hostName"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Host ID: try user.id or hostId
    let host_id = listing_data
        .get("user")
        .and_then(|u| u.get("id"))
        .or_else(|| listing_data.get("hostId"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });

    let url = format!("{base_url}/rooms/{id}");

    Some(Listing {
        id,
        name,
        location,
        price_per_night,
        currency,
        rating,
        review_count,
        thumbnail_url,
        property_type,
        host_name,
        host_id,
        url,
        is_superhost: None,
        is_guest_favorite: None,
        instant_book: None,
        total_price: None,
        photos: vec![],
        latitude: None,
        longitude: None,
    })
}

fn extract_price_legacy(data: &serde_json::Value) -> Option<f64> {
    // Try various price field locations
    if let Some(pq) = data.get("pricingQuote") {
        if let Some(price) = pq
            .get("price")
            .and_then(|p| p.get("amount"))
            .and_then(serde_json::Value::as_f64)
        {
            return Some(price);
        }
        if let Some(price) = pq
            .get("structuredStayDisplayPrice")
            .and_then(|s| s.get("primaryLine"))
            .and_then(|p| p.get("price"))
            .and_then(|p| p.as_str())
            .and_then(parse_price_amount)
        {
            return Some(price);
        }
    }

    data.get("price")
        .or_else(|| data.get("pricePerNight"))
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(parse_price_amount))
        })
}

fn find_pagination_cursor(data: &serde_json::Value) -> Option<String> {
    // Look for pagination info
    if let Some(cursor) = navigate_json(
        data,
        &[
            "data",
            "presentation",
            "staysSearch",
            "results",
            "paginationInfo",
            "nextPageCursor",
        ],
    ) {
        return cursor.as_str().map(String::from);
    }
    if let Some(cursor) = navigate_json(data, &["props", "pageProps", "pagination", "nextCursor"]) {
        return cursor.as_str().map(String::from);
    }
    None
}

fn parse_search_css(document: &Html, base_url: &str) -> Result<SearchResult> {
    let card_selector = Selector::parse(
        "[itemprop='itemListElement'], [data-testid='card-container']",
    )
    .map_err(|e| AirbnbError::Parse {
        reason: format!("invalid CSS selector: {e}"),
    })?;
    let link_selector = Selector::parse("a[href*='/rooms/']").map_err(|e| AirbnbError::Parse {
        reason: format!("invalid CSS selector: {e}"),
    })?;

    let mut seen = HashSet::new();
    let mut listings = Vec::new();
    // Select inside each card instead of re-serializing and re-parsing it.
    // Nested card containers yield the same link, so ids are de-duplicated.
    for card in document.select(&card_selector) {
        let Some(link) = card.select(&link_selector).next() else {
            continue;
        };
        let Some(id) = link.value().attr("href").and_then(extract_id_from_url) else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let name = link.text().collect::<String>().trim().to_string();
        listings.push(Listing {
            url: format!("{base_url}/rooms/{id}"),
            id,
            name: if name.is_empty() {
                "Untitled listing".to_string()
            } else {
                name
            },
            location: String::new(),
            price_per_night: 0.0,
            currency: String::new(),
            rating: None,
            review_count: 0,
            thumbnail_url: None,
            property_type: None,
            host_name: None,
            host_id: None,
            is_superhost: None,
            is_guest_favorite: None,
            instant_book: None,
            total_price: None,
            photos: vec![],
            latitude: None,
            longitude: None,
        });
    }

    if listings.is_empty() {
        return Err(AirbnbError::UpstreamSchema {
            operation: "search page".into(),
            detail: "no listing data and no listing cards in the page".into(),
        });
    }
    tracing::warn!(
        count = listings.len(),
        "CSS fallback produced listings with incomplete data (unknown price, no location)"
    );
    Ok(SearchResult {
        listings,
        total_count: None,
        next_cursor: None,
    })
}

fn extract_id_from_url(url: &str) -> Option<String> {
    // Extract listing ID from URLs like "/rooms/12345?..."
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "rooms"
            && let Some(id_part) = parts.get(i + 1)
        {
            let id = id_part.split('?').next().unwrap_or(id_part);
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_id_from_valid_url() {
        assert_eq!(
            extract_id_from_url("/rooms/12345?adults=2"),
            Some("12345".into())
        );
        assert_eq!(extract_id_from_url("/rooms/67890"), Some("67890".into()));
    }

    #[test]
    fn extract_id_from_invalid_url() {
        assert_eq!(extract_id_from_url("/search/results"), None);
    }

    #[test]
    fn parse_empty_html_returns_error() {
        let result = parse_search_results("<html><body></body></html>", "https://www.airbnb.com");
        assert!(result.is_err());
    }

    #[test]
    fn parse_next_data_json() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"searchResults":[
            {"listing":{"id":"123","name":"Test Place","city":"Paris","avgRating":4.8,"reviewsCount":10},"pricingQuote":{"price":{"amount":150.0}}}
        ]}}}
        </script></head><body></body></html>"#;

        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].id, "123");
        assert_eq!(result.listings[0].name, "Test Place");
        assert!((result.listings[0].price_per_night - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_deferred_state_search() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"props":{"pageProps":{"searchResults":[
            {"listing":{"id":"456","name":"Deferred Place","city":"London","avgRating":4.2,"reviewsCount":5},"pricingQuote":{"price":{"amount":80.0}}}
        ]}}}
        </script></head><body></body></html>"#;

        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].id, "456");
        assert_eq!(result.listings[0].name, "Deferred Place");
    }

    #[test]
    fn extract_listing_numeric_id() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":12345,"name":"Numeric ID","city":"Berlin","price":100.0},"pricingQuote":{"price":{"amount":100.0}}}"#
        ).unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com").unwrap();
        assert_eq!(listing.id, "12345");
    }

    #[test]
    fn extract_price_structured_display() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"pricingQuote":{"structuredStayDisplayPrice":{"primaryLine":{"price":"$150"}}}}"#,
        )
        .unwrap();
        let price = extract_price_legacy(&data).unwrap();
        assert!((price - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_price_from_string_field() {
        let data: serde_json::Value = serde_json::from_str(r#"{"price":"$200"}"#).unwrap();
        let price = extract_price_legacy(&data).unwrap();
        assert!((price - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pagination_cursor_extracted() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"data":{"presentation":{"staysSearch":{"results":{
            "searchResults":[
                {"listing":{"id":"1","name":"A","city":"X","price":50.0},"pricingQuote":{"price":{"amount":50.0}}}
            ],
            "paginationInfo":{"nextPageCursor":"cursor_abc"}
        }}}}}</script></head><body></body></html>"#;

        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.next_cursor, Some("cursor_abc".to_string()));
    }

    #[test]
    fn deep_find_listings_nested() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"wrapper":{"inner":[
                {"listing":{"id":"a","name":"Deep","city":"Z"},"pricingQuote":{"price":{"amount":75.0}}},
                {"listing":{"id":"b","name":"Deep2","city":"Z"},"pricingQuote":{"price":{"amount":80.0}}}
            ]}}"#
        ).unwrap();
        let results = deep_find_listings(&data, 20);
        assert!(results.is_some());
        assert_eq!(results.unwrap().len(), 2);
    }

    #[test]
    fn currency_extraction_from_pricing_quote() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":"1","name":"Euro Place","city":"Paris","currency":"EUR"},"pricingQuote":{"price":{"amount":100.0,"currencySymbol":"€"}}}"#
        ).unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com").unwrap();
        assert_eq!(listing.currency, "€");
    }

    #[test]
    fn currency_fallback_to_listing_field() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":"2","name":"GBP Place","city":"London","priceCurrency":"£"},"pricingQuote":{"price":{"amount":80.0}}}"#
        ).unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com").unwrap();
        assert_eq!(listing.currency, "£");
    }

    #[test]
    fn currency_unknown_stays_empty_until_the_client_labels_it() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":"3","name":"No Currency","city":"NYC"},"pricingQuote":{"price":{"amount":120.0}}}"#
        ).unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com").unwrap();
        assert_eq!(listing.currency, "");
        let iso: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":"4","name":"Iso","city":"NYC","currency":"USD"},"pricingQuote":{"price":{"amount":120.0}}}"#
        ).unwrap();
        assert_eq!(
            extract_listing_from_section(&iso, "https://www.airbnb.com")
                .unwrap()
                .currency,
            "$"
        );
    }

    #[test]
    fn deep_find_single_listing() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"wrapper":{"inner":[
                {"listing":{"id":"solo","name":"Only One","city":"Z"},"pricingQuote":{"price":{"amount":50.0}}}
            ]}}"#
        ).unwrap();
        let results = deep_find_listings(&data, 20);
        assert!(results.is_some());
        assert_eq!(results.unwrap().len(), 1);
    }

    #[test]
    fn deep_find_respects_max_depth() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"a":{"b":{"c":[{"listing":{"id":"deep","name":"Too Deep"}}]}}}"#,
        )
        .unwrap();
        let shallow = deep_find_listings(&data, 1);
        assert!(shallow.is_none());

        let deep = deep_find_listings(&data, 20);
        assert!(deep.is_some());
    }

    #[test]
    fn parse_css_fallback_listings() {
        let html = r#"<html><body>
        <div itemprop="itemListElement">
            <a href="/rooms/111?adults=1">Nice Room</a>
        </div>
        <div itemprop="itemListElement">
            <a href="/rooms/222">Another Room</a>
        </div>
        </body></html>"#;

        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 2);
        assert_eq!(result.listings[0].id, "111");
        assert_eq!(result.listings[1].id, "222");
    }

    #[test]
    fn parse_niobe_client_data_search() {
        // Simulates the current Airbnb format with niobeClientData wrapper
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"niobeClientData":[["StaysSearch:test",{
            "data":{"presentation":{"staysSearch":{"results":{
                "searchResults":[
                    {
                        "title":"Room in Paris",
                        "subtitle":"Cozy Studio near Eiffel Tower",
                        "avgRatingLocalized":"4.9 (42)",
                        "demandStayListing":{
                            "id":"RGVtYW5kU3RheUxpc3Rpbmc6MTIzNDU2Nzg5",
                            "location":{"coordinate":{"latitude":48.85,"longitude":2.29}}
                        },
                        "structuredDisplayPrice":{
                            "primaryLine":{"price":"€ 85","qualifier":"night"},
                            "explanationData":null
                        },
                        "contextualPictures":[{"picture":"https://example.com/photo.jpg"}],
                        "structuredContent":{"primaryLine":[{"body":"Hosted by Marie","type":"HOSTINFO"}]}
                    }
                ],
                "paginationInfo":{"nextPageCursor":"cursor_xyz"}
            }}}}
        }]]}
        </script></head><body></body></html>"#;

        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].id, "123456789");
        assert_eq!(result.listings[0].name, "Cozy Studio near Eiffel Tower");
        assert_eq!(result.listings[0].location, "Paris");
        assert!((result.listings[0].price_per_night - 85.0).abs() < f64::EPSILON);
        assert_eq!(result.listings[0].currency, "€");
        assert!((result.listings[0].rating.unwrap() - 4.9).abs() < f64::EPSILON);
        assert_eq!(result.listings[0].review_count, 42);
        assert_eq!(
            result.listings[0].thumbnail_url,
            Some("https://example.com/photo.jpg".to_string())
        );
        assert_eq!(result.listings[0].host_name, Some("Marie".to_string()));
        assert_eq!(result.next_cursor, Some("cursor_xyz".to_string()));
    }

    #[test]
    fn search_page_with_real_stay_search_results_uses_nightly_prices() {
        let payload = crate::test_helpers::fixture_json("p1b/stays_search.json");
        let html =
            crate::test_helpers::niobe_page("niobeClientData", &[("StaysSearch:{}", &payload)]);
        let result = parse_search_results(&html, "https://www.airbnb.com").unwrap();
        let prices: Vec<(String, Option<f64>, Option<f64>)> = result
            .listings
            .iter()
            .map(|l| (l.id.clone(), l.known_price(), l.total_price))
            .collect();
        assert_eq!(
            prices,
            vec![
                (
                    "1344016074576611955".to_string(),
                    Some(446.94),
                    Some(2245.0)
                ),
                ("968734101213290865".to_string(), Some(158.39), Some(797.0)),
                ("1532589801839552925".to_string(), Some(147.24), Some(810.0)),
            ]
        );
        assert!(result.listings.iter().all(|l| l.currency == "$"));
        assert_eq!(result.listings[2].name, "Example Hotel Lyon");
        assert_eq!(result.listings[2].location, "3rd Arrondissement");
        assert!(result.listings.iter().all(|l| l.host_name.is_none()));
    }

    #[test]
    fn legacy_listing_without_price_is_kept_with_unknown_price() {
        let data: serde_json::Value =
            serde_json::from_str(r#"{"listing":{"id":"77","name":"No Price Yet","city":"Lyon"}}"#)
                .unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com")
            .expect("an unpriced listing is still a listing");
        assert_eq!(listing.id, "77");
        assert_eq!(listing.known_price(), None);
    }

    #[test]
    fn deep_search_ignores_unrelated_id_arrays() {
        // No known search path: a decoy array of ids must not become listings.
        let decoy = serde_json::json!({
            "data": {"filters": [{"id": "x"}, {"id": "y"}]}
        });
        assert!(extract_listings_from_json(&decoy, "https://www.airbnb.com").is_none());

        // A priced legacy card found only by the deep search is still returned.
        let priced = serde_json::json!({
            "data": {"cards": [{"id": "1", "name": "Priced", "price": 120.0}]}
        });
        let result = extract_listings_from_json(&priced, "https://www.airbnb.com")
            .expect("a priced card found by the deep search is a listing");
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].known_price(), Some(120.0));
    }

    #[test]
    fn niobe_minimal_wrapper_is_unwrapped_like_niobe_client_data() {
        let payload = crate::test_helpers::fixture_json("p1b/stays_search.json");
        let html = crate::test_helpers::niobe_page(
            "niobeMinimalClientData",
            &[("StaysSearch:{}", &payload)],
        );
        let result = parse_search_results(&html, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 3);
        assert_eq!(result.listings[0].known_price(), Some(446.94));
    }

    #[test]
    fn nested_css_cards_are_listed_once() {
        let html = r#"<html><body>
            <div data-testid="card-container"><div data-testid="card-container"><a href="/rooms/111">Nested</a></div></div>
            <div itemprop="itemListElement"><a href="/rooms/222">Other</a></div>
        </body></html>"#;
        let result = parse_search_results(html, "https://www.airbnb.com").unwrap();
        let ids: Vec<&str> = result.listings.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["111", "222"]);
        assert!(
            result
                .listings
                .iter()
                .all(|l| l.currency.is_empty() && l.known_price().is_none())
        );
    }
}
