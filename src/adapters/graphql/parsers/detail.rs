use serde_json::Value;

use crate::domain::listing::ListingDetail;
use crate::error::{AirbnbError, Result};

/// Parse the GraphQL `StaysPdpSections` response into a `ListingDetail`.
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn parse_detail_response(json: &Value, id: &str, base_url: &str) -> Result<ListingDetail> {
    let sections = json
        .pointer("/data/presentation/stayProductDetailPage/sections/sections")
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::Parse {
            reason: "GraphQL detail: could not find sections array".into(),
        })?;

    let mut name = String::new();
    let mut location = String::new();
    let mut description = String::new();
    let mut price_per_night = 0.0;
    let mut currency = "USD".to_string();
    let mut rating: Option<f64> = None;
    let mut review_count: u32 = 0;
    let mut property_type: Option<String> = None;
    let mut host_name: Option<String> = None;
    let mut amenities = Vec::new();
    let mut house_rules = Vec::new();
    let mut photos = Vec::new();
    let mut bedrooms: Option<u32> = None;
    let mut beds: Option<u32> = None;
    let mut bathrooms: Option<f64> = None;
    let mut max_guests: Option<u32> = None;
    let mut check_in_time: Option<String> = None;
    let mut check_out_time: Option<String> = None;
    let mut host_id: Option<String> = None;
    let mut host_is_superhost: Option<bool> = None;
    let mut host_response_rate: Option<String> = None;
    let mut host_response_time: Option<String> = None;
    let mut host_joined: Option<String> = None;
    let mut host_total_listings: Option<u32> = None;
    let mut host_languages: Vec<String> = Vec::new();
    let mut cancellation_policy: Option<String> = None;
    let mut cleaning_fee: Option<f64> = None;
    let mut service_fee: Option<f64> = None;
    let mut neighborhood: Option<String> = None;
    let mut latitude: Option<f64> = None;
    let mut longitude: Option<f64> = None;

    for section in sections {
        let section_type = section
            .get("sectionComponentType")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let section_id = section
            .get("sectionId")
            .or_else(|| section.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        let data = section.get("section").unwrap_or(section);

        match section_type {
            "TITLE_DEFAULT" => {
                if name.is_empty()
                    && let Some(n) = data.get("title").and_then(Value::as_str)
                {
                    name = n.to_string();
                }
                if location.is_empty()
                    && let Some(subtitle) = data.get("subtitle").and_then(Value::as_str)
                {
                    location = subtitle.to_string();
                }
            }
            "HERO_DEFAULT" => {
                // Extract photos from hero previewImages
                if photos.is_empty()
                    && let Some(images) = data.get("previewImages").and_then(Value::as_array)
                {
                    for img in images {
                        if let Some(url) = img.get("baseUrl").and_then(Value::as_str) {
                            photos.push(url.to_string());
                        }
                    }
                }
            }
            "DESCRIPTION_DEFAULT" | "DESCRIPTION_SECTION" => {
                if let Some(d) = data
                    .pointer("/htmlDescription/htmlText")
                    .and_then(Value::as_str)
                {
                    description = strip_html_tags(d);
                } else if let Some(d) = data.get("description").and_then(Value::as_str) {
                    description = d.to_string();
                }
            }
            "AMENITIES_DEFAULT" | "AMENITIES_SECTION" => {
                // Try seeAllAmenitiesGroups first (complete list), then previewAmenitiesGroups, then amenityGroups
                let groups = data
                    .get("seeAllAmenitiesGroups")
                    .or_else(|| data.get("previewAmenitiesGroups"))
                    .or_else(|| data.get("amenityGroups"))
                    .and_then(Value::as_array);
                if let Some(groups) = groups {
                    for group in groups {
                        if let Some(items) = group.get("amenities").and_then(Value::as_array) {
                            for item in items {
                                if item.get("available").and_then(Value::as_bool) == Some(false) {
                                    continue;
                                }
                                if let Some(title) = item.get("title").and_then(Value::as_str)
                                    && !amenities.contains(&title.to_string())
                                {
                                    amenities.push(title.to_string());
                                }
                            }
                        }
                    }
                }
            }
            "POLICIES_DEFAULT" | "HOUSE_RULES_DEFAULT" => {
                if let Some(rules) = data.get("houseRules").and_then(Value::as_array) {
                    for rule in rules {
                        if let Some(title) = rule.get("title").and_then(Value::as_str) {
                            house_rules.push(title.to_string());
                        }
                    }
                }
                if let Some(policy) = data
                    .pointer("/cancellationPolicy/title")
                    .and_then(Value::as_str)
                {
                    cancellation_policy = Some(policy.to_string());
                }
            }
            "PHOTO_TOUR_SCROLLABLE" | "PHOTO_TOUR_MODAL" => {
                if let Some(media_items) = data.get("mediaItems").and_then(Value::as_array) {
                    for item in media_items {
                        if let Some(url) = item
                            .get("baseUrl")
                            .or_else(|| item.get("url"))
                            .and_then(Value::as_str)
                            && !photos.contains(&url.to_string())
                        {
                            photos.push(url.to_string());
                        }
                    }
                }
            }
            "BOOK_IT_SIDEBAR" => {
                // Pricing from sidebar — try multiple field name variants
                if let Some(n) = data
                    .pointer("/structuredStayDisplayPrice/primaryLine/price")
                    .or_else(|| data.pointer("/structuredDisplayPrice/primaryLine/discountedPrice"))
                    .or_else(|| data.pointer("/structuredDisplayPrice/primaryLine/originalPrice"))
                    .or_else(|| data.pointer("/structuredDisplayPrice/primaryLine/price"))
                    .and_then(Value::as_str)
                    .and_then(extract_price_number)
                {
                    price_per_night = n;
                }
                if price_per_night == 0.0
                    && let Some(p) = data.pointer("/price/amount").and_then(Value::as_f64)
                {
                    price_per_night = p;
                }
                // maxGuestCapacity from BOOK_IT_SIDEBAR
                if max_guests.is_none()
                    && let Some(cap) = data.get("maxGuestCapacity").and_then(Value::as_u64)
                {
                    max_guests = Some(cap as u32);
                }
            }
            "SBUI_SENTINEL"
                if section_id == "OVERVIEW_DEFAULT_V2" || section_id == "OVERVIEW_DEFAULT" =>
            {
                // Overview section has room counts in detailItems
                if let Some(detail_items) = data.get("detailItems").and_then(Value::as_array) {
                    for item in detail_items {
                        let title = item
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if title.contains("guest") {
                            max_guests = extract_number(title);
                        } else if title.contains("bedroom") {
                            bedrooms = extract_number(title);
                        } else if title.contains("bed") {
                            beds = extract_number(title);
                        } else if title.contains("bath") {
                            bathrooms = extract_number(title).map(f64::from);
                        }
                    }
                }
            }
            "OVERVIEW_DEFAULT" => {
                // Legacy overview section format
                if let Some(detail_items) = data.get("detailItems").and_then(Value::as_array) {
                    for item in detail_items {
                        let title = item
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if title.contains("guest") {
                            max_guests = extract_number(title);
                        } else if title.contains("bedroom") {
                            bedrooms = extract_number(title);
                        } else if title.contains("bed") {
                            beds = extract_number(title);
                        } else if title.contains("bath") {
                            bathrooms = extract_number(title).map(f64::from);
                        }
                    }
                }
            }
            "MEET_YOUR_HOST" | "HOST_PROFILE_DEFAULT" | "HOST_OVERVIEW_DEFAULT" => {
                // MEET_YOUR_HOST stores host info in cardData
                let card = data.get("cardData");
                host_name = card
                    .and_then(|c| c.get("name"))
                    .or_else(|| data.get("hostName"))
                    .or_else(|| data.get("name"))
                    .and_then(Value::as_str)
                    .map(String::from);
                host_id = card
                    .and_then(|c| c.get("userId"))
                    .or_else(|| data.get("hostId"))
                    .or_else(|| data.get("id"))
                    .and_then(|v| {
                        v.as_str()
                            .map(String::from)
                            .or_else(|| v.as_u64().map(|n| n.to_string()))
                    });
                host_is_superhost = card
                    .and_then(|c| c.get("isSuperhost"))
                    .or_else(|| data.get("isSuperhost"))
                    .and_then(Value::as_bool);
                // Response rate/time from hostDetails array or direct fields
                if let Some(details) = data.get("hostDetails").and_then(Value::as_array) {
                    for detail_str in details.iter().filter_map(Value::as_str) {
                        let lower = detail_str.to_lowercase();
                        if lower.contains("response rate") {
                            host_response_rate = Some(detail_str.to_string());
                        } else if lower.contains("respond") {
                            host_response_time = Some(detail_str.to_string());
                        }
                    }
                }
                if host_response_rate.is_none() {
                    host_response_rate = data
                        .get("hostResponseRate")
                        .and_then(Value::as_str)
                        .map(String::from);
                }
                if host_response_time.is_none() {
                    host_response_time = data
                        .get("hostRespondTimeCopy")
                        .or_else(|| data.get("hostResponseTime"))
                        .and_then(Value::as_str)
                        .map(String::from);
                }
                host_joined = data
                    .get("hostMemberSince")
                    .and_then(Value::as_str)
                    .map(String::from);
                host_total_listings = data
                    .get("hostListingCount")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32);
                // Languages from hostHighlights
                if host_languages.is_empty() {
                    if let Some(langs) = data.get("hostLanguages").and_then(Value::as_array) {
                        host_languages = langs
                            .iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect();
                    }
                    if host_languages.is_empty() {
                        host_languages = extract_languages_from_highlights(data);
                    }
                }
            }
            "LOCATION_DEFAULT" | "LOCATION_PDP" => {
                if location.is_empty()
                    && let Some(loc) = data.get("subtitle").and_then(Value::as_str)
                {
                    location = loc.to_string();
                }
                if location.is_empty()
                    && let Some(loc) = data.get("title").and_then(Value::as_str)
                {
                    location = loc.to_string();
                }
                latitude = data.get("lat").and_then(Value::as_f64);
                longitude = data.get("lng").and_then(Value::as_f64);
                if neighborhood.is_none() {
                    neighborhood = data
                        .get("subtitle")
                        .and_then(Value::as_str)
                        .map(String::from);
                }
            }
            "REVIEWS_DEFAULT" => {
                if rating.is_none() {
                    rating = data.get("overallRating").and_then(Value::as_f64);
                }
                if review_count == 0 {
                    review_count = data
                        .get("overallCount")
                        .or_else(|| data.get("reviewsCount"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                }
            }
            _ => {
                // Extract rating from any section containing review info
                if rating.is_none() {
                    rating = data
                        .get("overallRating")
                        .and_then(Value::as_f64)
                        .or_else(|| {
                            data.pointer("/reviewSummary/overallRating")
                                .and_then(Value::as_f64)
                        });
                }
                if review_count == 0 {
                    review_count = data
                        .get("overallCount")
                        .or_else(|| data.get("reviewsCount"))
                        .or_else(|| data.pointer("/reviewSummary/totalReviews"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                }
                if property_type.is_none() {
                    property_type = data
                        .get("propertyType")
                        .or_else(|| data.get("roomType"))
                        .and_then(Value::as_str)
                        .map(String::from);
                }
            }
        }
    }

    // Also check for pricing in the top-level metadata
    if price_per_night == 0.0
        && let Some(p) = json
            .pointer("/data/presentation/stayProductDetailPage/sections/metadata/loggingContext/eventDataLogging/listingPrice")
            .and_then(Value::as_f64)
    {
        price_per_night = p;
    }

    // Extract fees from pricing breakdown
    if let Some(breakdown) = json.pointer(
        "/data/presentation/stayProductDetailPage/sections/metadata/bookingPrefetchData/priceBreakdown/priceItems",
    ).and_then(Value::as_array) {
        for item in breakdown {
            let label = item.get("localizedTitle").and_then(Value::as_str).unwrap_or_default();
            let amount = item.pointer("/total/amountMicros").and_then(Value::as_f64).map(|m| m / 1_000_000.0)
                .or_else(|| item.pointer("/total/amount").and_then(Value::as_f64));
            if label.to_lowercase().contains("cleaning") {
                cleaning_fee = amount;
            } else if label.to_lowercase().contains("service") {
                service_fee = amount;
            }
        }
    }

    if let Some(c) = json
        .pointer("/data/presentation/stayProductDetailPage/sections/metadata/loggingContext/eventDataLogging/currency")
        .and_then(Value::as_str)
    {
        currency = c.to_string();
    }

    if let Some(ci) = json
        .pointer("/data/presentation/stayProductDetailPage/sections/metadata/bookingPrefetchData/checkIn")
        .and_then(Value::as_str)
    {
        check_in_time = Some(ci.to_string());
    }

    if let Some(co) = json
        .pointer("/data/presentation/stayProductDetailPage/sections/metadata/bookingPrefetchData/checkOut")
        .and_then(Value::as_str)
    {
        check_out_time = Some(co.to_string());
    }

    let url = format!("{base_url}/rooms/{id}");

    Ok(ListingDetail {
        id: id.to_string(),
        name,
        location,
        description,
        price_per_night,
        currency,
        rating,
        review_count,
        property_type,
        host_name,
        url,
        amenities,
        house_rules,
        latitude,
        longitude,
        photos,
        bedrooms,
        beds,
        bathrooms,
        max_guests,
        check_in_time,
        check_out_time,
        host_id,
        host_is_superhost,
        host_response_rate,
        host_response_time,
        host_joined,
        host_total_listings,
        host_languages,
        cancellation_policy,
        instant_book: None,
        cleaning_fee,
        service_fee,
        neighborhood,
    })
}

/// Extract a number from a string like "3 bedrooms" -> 3.
fn extract_number(s: &str) -> Option<u32> {
    s.split_whitespace().find_map(|word| word.parse().ok())
}

/// Extract a price number from strings like "$120", "€95.50".
fn extract_price_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cleaned.parse().ok()
}

/// Extract languages from hostHighlights like "Speaks English and French".
fn extract_languages_from_highlights(data: &Value) -> Vec<String> {
    let Some(highlights) = data.get("hostHighlights").and_then(Value::as_array) else {
        return Vec::new();
    };
    for highlight in highlights {
        if let Some(title) = highlight.get("title").and_then(Value::as_str) {
            let lower = title.to_lowercase();
            if lower.starts_with("speaks ") {
                return title[7..]
                    .split([',', '&'])
                    .flat_map(|s| s.split(" and "))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    Vec::new()
}

/// Minimal HTML tag stripping.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_basic() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn extract_number_from_text() {
        assert_eq!(extract_number("3 bedrooms"), Some(3));
        assert_eq!(extract_number("studio"), None);
    }

    #[test]
    fn parse_detail_with_all_sections() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "sections": {
                            "sections": [
                                {
                                    "sectionComponentType": "TITLE_DEFAULT",
                                    "sectionId": "TITLE_DEFAULT",
                                    "section": { "title": "Grand Villa", "subtitle": "Malibu, CA" }
                                },
                                {
                                    "sectionComponentType": "DESCRIPTION_DEFAULT",
                                    "sectionId": "DESCRIPTION_DEFAULT",
                                    "section": { "description": "A beautiful villa by the sea" }
                                },
                                {
                                    "sectionComponentType": "AMENITIES_DEFAULT",
                                    "sectionId": "AMENITIES_DEFAULT",
                                    "section": { "seeAllAmenitiesGroups": [{ "amenities": [{ "title": "Pool", "available": true }, { "title": "WiFi", "available": true }] }] }
                                },
                                {
                                    "sectionComponentType": "POLICIES_DEFAULT",
                                    "sectionId": "POLICIES_DEFAULT",
                                    "section": {
                                        "houseRules": [{ "title": "No parties" }],
                                        "cancellationPolicy": { "title": "Flexible" }
                                    }
                                },
                                {
                                    "sectionComponentType": "HERO_DEFAULT",
                                    "sectionId": "HERO_DEFAULT",
                                    "section": { "previewImages": [{ "baseUrl": "https://img.example.com/1.jpg" }] }
                                },
                                {
                                    "sectionComponentType": "SBUI_SENTINEL",
                                    "sectionId": "OVERVIEW_DEFAULT_V2",
                                    "section": {
                                        "detailItems": [
                                            { "title": "4 guests" },
                                            { "title": "2 bedrooms" },
                                            { "title": "3 beds" },
                                            { "title": "2 bathrooms" }
                                        ]
                                    }
                                },
                                {
                                    "sectionComponentType": "MEET_YOUR_HOST",
                                    "sectionId": "MEET_YOUR_HOST",
                                    "section": {
                                        "cardData": { "name": "Alice", "userId": "555", "isSuperhost": true },
                                        "hostDetails": ["Response rate: 98%", "Responds within an hour"],
                                        "hostHighlights": [{ "title": "Speaks English and French" }]
                                    }
                                },
                                {
                                    "sectionComponentType": "LOCATION_PDP",
                                    "sectionId": "LOCATION_DEFAULT",
                                    "section": { "lat": 34.03, "lng": -118.77, "subtitle": "Malibu Coast" }
                                },
                                {
                                    "sectionComponentType": "REVIEWS_DEFAULT",
                                    "sectionId": "REVIEWS_DEFAULT",
                                    "section": { "overallRating": 4.85, "overallCount": 100 }
                                }
                            ],
                            "metadata": {}
                        }
                    }
                }
            }
        });
        let detail = parse_detail_response(&json, "100", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Grand Villa");
        assert_eq!(detail.location, "Malibu, CA");
        assert_eq!(detail.description, "A beautiful villa by the sea");
        assert!(detail.amenities.contains(&"Pool".to_string()));
        assert!(detail.amenities.contains(&"WiFi".to_string()));
        assert!(detail.house_rules.contains(&"No parties".to_string()));
        assert_eq!(detail.cancellation_policy, Some("Flexible".into()));
        assert_eq!(detail.photos.len(), 1);
        assert_eq!(detail.max_guests, Some(4));
        assert_eq!(detail.bedrooms, Some(2));
        assert_eq!(detail.beds, Some(3));
        assert_eq!(detail.bathrooms, Some(2.0));
        assert_eq!(detail.host_name, Some("Alice".into()));
        assert_eq!(detail.host_id, Some("555".into()));
        assert_eq!(detail.host_is_superhost, Some(true));
        assert_eq!(detail.host_response_rate, Some("Response rate: 98%".into()));
        assert_eq!(
            detail.host_response_time,
            Some("Responds within an hour".into())
        );
        assert_eq!(detail.host_languages, vec!["English", "French"]);
        assert!((detail.latitude.unwrap() - 34.03).abs() < 0.01);
        assert_eq!(detail.neighborhood, Some("Malibu Coast".into()));
        assert!((detail.rating.unwrap() - 4.85).abs() < 0.01);
        assert_eq!(detail.review_count, 100);
    }

    #[test]
    fn parse_detail_with_fees() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "sections": {
                            "sections": [{
                                "sectionComponentType": "TITLE_DEFAULT",
                                "section": { "title": "Test", "subtitle": "Test City" }
                            }],
                            "metadata": {
                                "bookingPrefetchData": {
                                    "priceBreakdown": {
                                        "priceItems": [
                                            { "localizedTitle": "Cleaning fee", "total": { "amount": 50.0 } },
                                            { "localizedTitle": "Service fee", "total": { "amountMicros": 30_000_000.0 } }
                                        ]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let detail = parse_detail_response(&json, "200", "https://www.airbnb.com").unwrap();
        assert!((detail.cleaning_fee.unwrap() - 50.0).abs() < 0.01);
        assert!((detail.service_fee.unwrap() - 30.0).abs() < 0.01);
    }

    #[test]
    fn parse_minimal_detail() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "sections": {
                            "sections": [{
                                "sectionComponentType": "TITLE_DEFAULT",
                                "section": {
                                    "title": "Cozy Place",
                                    "subtitle": "Paris, France"
                                }
                            }],
                            "metadata": {}
                        }
                    }
                }
            }
        });

        let detail = parse_detail_response(&json, "12345", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.id, "12345");
        assert_eq!(detail.name, "Cozy Place");
        assert_eq!(detail.location, "Paris, France");
        assert_eq!(detail.url, "https://www.airbnb.com/rooms/12345");
    }
}
