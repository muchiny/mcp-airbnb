use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use scraper::{Html, Selector};

use crate::domain::listing::{Listing, SearchResult};
use crate::error::{AirbnbError, Result};

/// Extract search results from Airbnb HTML.
/// Strategy: try `__NEXT_DATA__` JSON first, then `data-deferred-state` (with niobeClientData),
/// fall back to CSS selectors.
pub fn parse_search_results(html: &str, base_url: &str) -> Result<SearchResult> {
    // Try __NEXT_DATA__ JSON extraction first (legacy, more reliable when present)
    if let Some(result) = try_parse_next_data_search(html, base_url) {
        return Ok(result);
    }

    // Try deferred state (current Airbnb format with niobeClientData)
    if let Some(result) = try_parse_deferred_state(html, base_url) {
        return Ok(result);
    }

    // Final fallback: CSS selectors
    parse_search_css(html, base_url)
}

fn try_parse_next_data_search(html: &str, base_url: &str) -> Option<SearchResult> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r"script#__NEXT_DATA__").ok()?;
    let script = document.select(&selector).next()?;
    let json_text = script.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&json_text).ok()?;

    extract_listings_from_json(&data, base_url)
}

fn try_parse_deferred_state(html: &str, base_url: &str) -> Option<SearchResult> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']").ok()?;

    for script in document.select(&selector) {
        let json_text = script.text().collect::<String>();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text) {
            // Try niobeClientData wrapper (current Airbnb format)
            if let Some(entries) = data.get("niobeClientData").and_then(|v| v.as_array()) {
                for entry in entries {
                    if let Some(inner) = entry.as_array().and_then(|arr| arr.get(1))
                        && let Some(result) = extract_listings_from_json(inner, base_url)
                    {
                        return Some(result);
                    }
                }
            }
            // Legacy: try direct JSON structure
            if let Some(result) = extract_listings_from_json(&data, base_url) {
                return Some(result);
            }
        }
    }
    None
}

fn extract_listings_from_json(data: &serde_json::Value, base_url: &str) -> Option<SearchResult> {
    // Navigate through various known JSON structures
    let sections = find_search_sections(data)?;
    let mut listings = Vec::new();

    for section in sections {
        if let Some(listing) = extract_listing_from_section(section, base_url) {
            listings.push(listing);
        }
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

fn find_search_sections(data: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    // Airbnb structures data in various nested paths
    let paths: &[&[&str]] = &[
        &["props", "pageProps", "searchResults"],
        &["niobeMinimalClientData"],
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

    // Deep search: look for arrays containing listing-like objects
    if let Some(results) = deep_find_listings(data, 20) {
        return Some(results);
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
    // Try new format first (niobeClientData / StaySearchResult)
    if section.get("demandStayListing").is_some() || section.get("structuredDisplayPrice").is_some()
    {
        return extract_listing_niobe_format(section, base_url);
    }

    // Legacy format: listing nested under "listing" key or flat
    extract_listing_legacy_format(section, base_url)
}

/// Extract listing from current Airbnb format (niobeClientData / `StaySearchResult`)
#[allow(clippy::too_many_lines)]
fn extract_listing_niobe_format(section: &serde_json::Value, base_url: &str) -> Option<Listing> {
    // Extract ID from demandStayListing.id (base64-encoded "DemandStayListing:NUMERIC_ID")
    let id = section
        .get("demandStayListing")
        .and_then(|dsl| dsl.get("id"))
        .and_then(|v| v.as_str())
        .and_then(decode_niobe_id)?;

    // Name: prefer subtitle (listing name), fall back to title (location-based)
    let name = section
        .get("subtitle")
        .and_then(|v| v.as_str())
        .or_else(|| {
            section
                .get("nameLocalized")
                .and_then(|n| n.get("localizedStringWithTranslationPreference"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| section.get("title").and_then(|v| v.as_str()))
        .unwrap_or("Unknown listing")
        .to_string();

    // Location: from title (e.g., "Place to stay in Paris" → "Paris") or demandStayListing
    let location = section
        .get("title")
        .and_then(|v| v.as_str())
        .map(extract_location_from_title)
        .unwrap_or_default();

    // Price: try to get per-night from explanation data, fall back to total
    let price_per_night = extract_price_niobe(section).unwrap_or(0.0);

    // Currency: extract from price string
    let currency = section
        .get("structuredDisplayPrice")
        .and_then(|sdp| sdp.get("primaryLine"))
        .and_then(|pl| pl.get("price"))
        .and_then(|v| v.as_str())
        .and_then(extract_currency_symbol)
        .unwrap_or_else(|| "$".to_string());

    // Rating: parse from avgRatingLocalized (e.g., "5.0 (5)")
    let (rating, review_count) = parse_avg_rating_localized(
        section
            .get("avgRatingLocalized")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );

    // Thumbnail
    let thumbnail_url = section
        .get("contextualPictures")
        .and_then(|pics| pics.as_array())
        .and_then(|arr| arr.first())
        .and_then(|pic| pic.get("picture"))
        .and_then(|p| p.as_str())
        .map(String::from);

    // Property type: extract from title pattern
    let property_type = extract_property_type_from_title(
        section.get("title").and_then(|v| v.as_str()).unwrap_or(""),
    );

    // Host name: from structuredContent primaryLine HOSTINFO
    let host_name = section
        .get("structuredContent")
        .and_then(|sc| sc.get("primaryLine"))
        .and_then(|pl| pl.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|item| {
                if item.get("type").and_then(|v| v.as_str()) == Some("HOSTINFO") {
                    item.get("body").and_then(|v| v.as_str()).map(String::from)
                } else {
                    None
                }
            })
        });

    let url = format!("{base_url}/rooms/{id}");

    // All photos from contextualPictures
    let photos = section
        .get("contextualPictures")
        .and_then(|pics| pics.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|pic| {
                    pic.get("picture")
                        .and_then(|p| p.as_str())
                        .map(String::from)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Coordinates from demandStayListing.location.coordinate
    let dsl = section.get("demandStayListing");
    let coord = dsl
        .and_then(|d| d.get("location"))
        .and_then(|loc| loc.get("coordinate"));
    let latitude = coord
        .and_then(|c| c.get("latitude"))
        .and_then(serde_json::Value::as_f64);
    let longitude = coord
        .and_then(|c| c.get("longitude"))
        .and_then(serde_json::Value::as_f64);

    // Superhost: check badges or structuredContent
    let is_superhost = section
        .get("badges")
        .and_then(|b| b.as_array())
        .and_then(|arr| {
            if arr.iter().any(|badge| {
                badge
                    .get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t.contains("SUPERHOST"))
            }) {
                Some(true)
            } else {
                None
            }
        })
        .or_else(|| {
            section
                .get("structuredContent")
                .and_then(|sc| sc.get("primaryLine"))
                .and_then(|pl| pl.as_array())
                .and_then(|arr| {
                    if arr.iter().any(|item| {
                        item.get("body")
                            .and_then(|v| v.as_str())
                            .is_some_and(|s| s.contains("Superhost"))
                    }) {
                        Some(true)
                    } else {
                        None
                    }
                })
        });

    // Guest Favorite badge
    let is_guest_favorite = section
        .get("guestFavorite")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            section
                .get("badges")
                .and_then(|b| b.as_array())
                .and_then(|arr| {
                    if arr.iter().any(|badge| {
                        badge
                            .get("type")
                            .and_then(|v| v.as_str())
                            .is_some_and(|t| t.contains("GUEST_FAVORITE"))
                    }) {
                        Some(true)
                    } else {
                        None
                    }
                })
        });

    // Instant book
    let instant_book = dsl
        .and_then(|d| d.get("instantBookEnabled"))
        .and_then(serde_json::Value::as_bool);

    // Total price from secondaryLine
    let total_price = section
        .get("structuredDisplayPrice")
        .and_then(|sdp| sdp.get("secondaryLine"))
        .and_then(|sl| sl.get("price"))
        .and_then(|v| v.as_str())
        .and_then(|s| {
            let digits: String = s
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            digits.parse::<f64>().ok()
        });

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
        url,
        is_superhost,
        is_guest_favorite,
        instant_book,
        total_price,
        photos,
        latitude,
        longitude,
    })
}

/// Decode base64 niobeClientData ID (e.g., "`RGVtYW5kU3RheUxpc3Rpbmc6MTI5MDE`..." → "1290194...")
fn decode_niobe_id(encoded: &str) -> Option<String> {
    let bytes = STANDARD.decode(encoded).ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    // Format: "DemandStayListing:NUMERIC_ID"
    decoded.split(':').nth(1).map(String::from)
}

/// Extract location from title like "Place to stay in Paris" → "Paris"
fn extract_location_from_title(title: &str) -> String {
    // Common patterns: "X in City", "X in City, Country"
    if let Some(idx) = title.rfind(" in ") {
        return title[(idx + 4)..].to_string();
    }
    title.to_string()
}

/// Extract per-night price from structured display price
fn extract_price_niobe(section: &serde_json::Value) -> Option<f64> {
    let sdp = section.get("structuredDisplayPrice")?;

    // Try to get per-night from explanation data ("X nights x €Y")
    if let Some(per_night) = sdp
        .get("explanationData")
        .and_then(|ed| ed.get("priceDetails"))
        .and_then(|pd| pd.as_array())
        .and_then(|arr| arr.first())
        .and_then(|group| group.get("items"))
        .and_then(|items| items.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("description"))
        .and_then(|v| v.as_str())
        .and_then(extract_per_night_from_description)
    {
        return Some(per_night);
    }

    // Fall back to primary line price
    let price_str = sdp
        .get("primaryLine")
        .and_then(|pl| pl.get("price"))
        .and_then(|v| v.as_str())?;

    parse_price_string(price_str)
}

/// Parse "5 nights x € 45.14" → 45.14
fn extract_per_night_from_description(desc: &str) -> Option<f64> {
    // Match pattern: "N nights x SYMBOL PRICE" or "N nights x PRICE"
    if let Some(idx) = desc.find(" x ") {
        let price_part = &desc[(idx + 3)..];
        return parse_price_string(price_part);
    }
    None
}

/// Parse a price string like "€ 254", "$150", "¥12000" into a float
fn parse_price_string(s: &str) -> Option<f64> {
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse::<f64>().ok()
}

/// Extract currency symbol from a price string like "€ 254" → "€"
fn extract_currency_symbol(price: &str) -> Option<String> {
    let symbol: String = price
        .chars()
        .take_while(|c| !c.is_ascii_digit())
        .collect::<String>()
        .trim()
        .to_string();
    if symbol.is_empty() {
        None
    } else {
        Some(symbol)
    }
}

/// Parse "5.0 (5)" → (Some(5.0), 5)
fn parse_avg_rating_localized(s: &str) -> (Option<f64>, u32) {
    if s.is_empty() {
        return (None, 0);
    }
    // Format: "4.98 (126)" or "New"
    let rating = s
        .split([' ', '('])
        .next()
        .and_then(|r| r.parse::<f64>().ok());

    let review_count = s
        .split('(')
        .nth(1)
        .and_then(|part| part.trim_end_matches(')').parse::<u32>().ok())
        .unwrap_or(0);

    (rating, review_count)
}

/// Extract property type from title like "Room in Paris" → "Private room"
fn extract_property_type_from_title(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    if lower.starts_with("room in") || lower.starts_with("place to stay") {
        Some("Private room".to_string())
    } else if lower.starts_with("apartment in")
        || lower.starts_with("home in")
        || lower.starts_with("condo in")
        || lower.starts_with("loft in")
        || lower.starts_with("townhouse in")
        || lower.starts_with("villa in")
        || lower.starts_with("rental unit in")
    {
        Some("Entire home".to_string())
    } else if lower.starts_with("hotel") {
        Some("Hotel".to_string())
    } else {
        None
    }
}

/// Extract listing from legacy Airbnb format (__`NEXT_DATA`__ style)
#[allow(clippy::cast_possible_truncation)]
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

    let price_per_night =
        extract_price_legacy(section).or_else(|| extract_price_legacy(listing_data))?;

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
        .unwrap_or("$")
        .to_string();

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
            .and_then(parse_price_string)
        {
            return Some(price);
        }
    }

    data.get("price")
        .or_else(|| data.get("pricePerNight"))
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(parse_price_string))
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

fn parse_search_css(html: &str, base_url: &str) -> Result<SearchResult> {
    let document = Html::parse_document(html);

    // Airbnb listing cards typically have data-testid or itemprop attributes
    let card_selector = Selector::parse(
        "[itemprop='itemListElement'], [data-testid='card-container']",
    )
    .map_err(|e| AirbnbError::Parse {
        reason: format!("invalid CSS selector: {e}"),
    })?;

    let mut listings = Vec::new();

    for card in document.select(&card_selector) {
        let card_html = card.html();
        let card_doc = Html::parse_fragment(&card_html);

        // Extract link with listing ID
        if let Ok(link_sel) = Selector::parse("a[href*='/rooms/']")
            && let Some(link) = card_doc.select(&link_sel).next()
            && let Some(href) = link.value().attr("href")
            && let Some(id) = extract_id_from_url(href)
        {
            let name = link.text().collect::<String>().trim().to_string();
            let name = if name.is_empty() {
                "Untitled listing".to_string()
            } else {
                name
            };

            listings.push(Listing {
                id: id.clone(),
                name,
                location: String::new(),
                price_per_night: 0.0,
                currency: "$".into(),
                rating: None,
                review_count: 0,
                thumbnail_url: None,
                property_type: None,
                host_name: None,
                url: format!("{base_url}/rooms/{id}"),
                is_superhost: None,
                is_guest_favorite: None,
                instant_book: None,
                total_price: None,
                photos: vec![],
                latitude: None,
                longitude: None,
            });
        }
    }

    if !listings.is_empty() {
        tracing::warn!(
            count = listings.len(),
            "CSS fallback produced listings with incomplete data (price=0, no location)"
        );
    }

    if listings.is_empty() {
        return Err(AirbnbError::Parse {
            reason: "no listings found in search results".into(),
        });
    }

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
    fn currency_defaults_to_dollar() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"listing":{"id":"3","name":"No Currency","city":"NYC"},"pricingQuote":{"price":{"amount":120.0}}}"#
        ).unwrap();
        let listing = extract_listing_from_section(&data, "https://www.airbnb.com").unwrap();
        assert_eq!(listing.currency, "$");
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
        assert_eq!(
            result.listings[0].host_name,
            Some("Hosted by Marie".to_string())
        );
        assert_eq!(result.next_cursor, Some("cursor_xyz".to_string()));
    }

    #[test]
    fn decode_niobe_id_valid() {
        // "DemandStayListing:123456789" base64 encoded
        let encoded = STANDARD.encode("DemandStayListing:123456789");
        assert_eq!(decode_niobe_id(&encoded), Some("123456789".to_string()));
    }

    #[test]
    fn parse_avg_rating_localized_full() {
        let (rating, count) = parse_avg_rating_localized("4.98 (126)");
        assert!((rating.unwrap() - 4.98).abs() < f64::EPSILON);
        assert_eq!(count, 126);
    }

    #[test]
    fn parse_avg_rating_localized_empty() {
        let (rating, count) = parse_avg_rating_localized("");
        assert!(rating.is_none());
        assert_eq!(count, 0);
    }

    #[test]
    fn parse_avg_rating_localized_new() {
        let (rating, count) = parse_avg_rating_localized("New");
        assert!(rating.is_none());
        assert_eq!(count, 0);
    }

    #[test]
    fn extract_per_night_from_desc() {
        assert!(
            (extract_per_night_from_description("5 nights x € 45.14").unwrap() - 45.14).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn extract_location_from_title_works() {
        assert_eq!(
            extract_location_from_title("Place to stay in Paris"),
            "Paris"
        );
        assert_eq!(
            extract_location_from_title("Room in London, UK"),
            "London, UK"
        );
    }

    #[test]
    fn extract_currency_symbol_works() {
        assert_eq!(extract_currency_symbol("€ 254"), Some("€".to_string()));
        assert_eq!(extract_currency_symbol("$150"), Some("$".to_string()));
    }
}
