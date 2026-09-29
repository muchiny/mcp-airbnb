use scraper::{Html, Selector};
use serde_json::Value;

use crate::adapters::graphql::parsers::{detail as pdp_detail, host as pdp_host};
use crate::adapters::price::normalize_currency;
use crate::adapters::scraper::deferred_state::{
    deferred_state_json, next_data_json, niobe_payloads,
};
use crate::domain::analytics::HostProfile;
use crate::domain::listing::ListingDetail;
use crate::error::{AirbnbError, Result};

use super::page_markers;

/// Everything the scraper extracts from one `/rooms/{id}` page.
pub struct ListingPage {
    /// The listing detail, or why it could not be extracted.
    pub detail: Result<ListingDetail>,
    /// The host profile, or why it could not be extracted.
    pub host: Result<HostProfile>,
}

/// Parse a listing page once and extract both the detail and the host profile.
///
/// Detail tiers: legacy `__NEXT_DATA__`; the embedded `StaysPdpSections`
/// payload (`niobeClientData` / `niobeMinimalClientData`), parsed by the same
/// code as the GraphQL response; heuristic JSON search over every payload;
/// CSS selectors. The structured payload is looked for in every entry before
/// any heuristic runs.
pub fn parse_listing_page(html: &str, listing_id: &str, base_url: &str) -> ListingPage {
    let document = Html::parse_document(html);
    let next_data = next_data_json(&document);
    let states = deferred_state_json(&document);
    let payloads: Vec<&Value> = states.iter().flat_map(niobe_payloads).collect();
    let pdp = payloads
        .iter()
        .copied()
        .find(|payload| is_pdp_sections(payload));

    let detail = next_data
        .as_ref()
        .and_then(|data| extract_detail_from_json(data, listing_id, base_url))
        .or_else(|| {
            pdp.and_then(|payload| {
                pdp_detail::parse_detail_response(payload, listing_id, base_url).ok()
            })
        })
        .or_else(|| {
            payloads
                .iter()
                .copied()
                .chain(states.iter())
                .find_map(|data| extract_detail_from_json(data, listing_id, base_url))
        })
        .map_or_else(
            || parse_detail_css(&document, html, listing_id, base_url),
            Ok,
        );

    ListingPage {
        detail,
        host: host_from_pdp(pdp),
    }
}

/// Parse listing detail page HTML into a `ListingDetail`.
pub fn parse_listing_detail(html: &str, listing_id: &str, base_url: &str) -> Result<ListingDetail> {
    parse_listing_page(html, listing_id, base_url).detail
}

/// Parse the host profile from a listing page HTML.
///
/// Returns [`AirbnbError::HostProfileUnavailable`] for business listings and
/// [`AirbnbError::UpstreamSchema`] when the page embeds no `StaysPdpSections`
/// payload.
pub fn parse_host_profile(html: &str) -> Result<HostProfile> {
    let document = Html::parse_document(html);
    let states = deferred_state_json(&document);
    host_from_pdp(
        states
            .iter()
            .flat_map(niobe_payloads)
            .find(|payload| is_pdp_sections(payload)),
    )
}

fn host_from_pdp(pdp: Option<&Value>) -> Result<HostProfile> {
    pdp.map_or_else(
        || {
            Err(AirbnbError::UpstreamSchema {
                operation: "listing page".into(),
                detail: "no StaysPdpSections data in the page".into(),
            })
        },
        pdp_host::parse_host_response,
    )
}

fn is_pdp_sections(payload: &Value) -> bool {
    payload
        .pointer("/data/presentation/stayProductDetailPage/sections/sections")
        .is_some_and(Value::is_array)
}

#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
fn extract_detail_from_json(
    data: &serde_json::Value,
    listing_id: &str,
    base_url: &str,
) -> Option<ListingDetail> {
    // Try various known JSON paths
    let listing = find_listing_data(data)?;

    let name = listing
        .get("name")
        .or_else(|| listing.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown listing")
        .to_string();

    let location = listing
        .get("location")
        .or_else(|| listing.get("city"))
        .or_else(|| listing.get("publicAddress"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let description = listing
        .get("description")
        .or_else(|| {
            listing
                .get("sectionedDescription")
                .and_then(|s| s.get("description"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let price_per_night = listing
        .get("price")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            listing
                .get("pricingQuote")
                .and_then(|pq| pq.get("price"))
                .and_then(|p| p.get("amount"))
                .and_then(serde_json::Value::as_f64)
        })
        .unwrap_or(0.0);

    let currency = listing
        .get("priceCurrency")
        .and_then(|v| v.as_str())
        .map(normalize_currency)
        .unwrap_or_default();

    let rating = listing
        .get("avgRating")
        .or_else(|| listing.get("overallRating"))
        .and_then(serde_json::Value::as_f64);

    let review_count = listing
        .get("reviewsCount")
        .or_else(|| listing.get("visibleReviewCount"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;

    let property_type = listing
        .get("roomType")
        .or_else(|| listing.get("propertyType"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let host_name = listing
        .get("host")
        .and_then(|h| h.get("name"))
        .or_else(|| listing.get("primaryHost").and_then(|h| h.get("firstName")))
        .and_then(|v| v.as_str())
        .map(String::from);

    let amenities = listing
        .get("amenities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("name")
                        .or_else(|| item.get("tag"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .or_else(|| item.as_str().map(String::from))
                })
                .collect()
        })
        .unwrap_or_default();

    let house_rules = listing
        .get("houseRules")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let latitude = listing
        .get("lat")
        .or_else(|| listing.get("latitude"))
        .and_then(serde_json::Value::as_f64);

    let longitude = listing
        .get("lng")
        .or_else(|| listing.get("longitude"))
        .and_then(serde_json::Value::as_f64);

    let photos = listing
        .get("photos")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("pictureUrl")
                        .or_else(|| item.get("baseUrl"))
                        .or_else(|| item.get("url"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .or_else(|| item.as_str().map(String::from))
                })
                .collect()
        })
        .unwrap_or_default();

    let bedrooms = listing
        .get("bedrooms")
        .or_else(|| listing.get("bedroomCount"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let beds = listing
        .get("beds")
        .or_else(|| listing.get("bedCount"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let bathrooms = listing
        .get("bathrooms")
        .or_else(|| listing.get("bathroomCount"))
        .and_then(serde_json::Value::as_f64);

    let max_guests = listing
        .get("personCapacity")
        .or_else(|| listing.get("maxGuests"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let check_in_time = listing
        .get("checkIn")
        .or_else(|| listing.get("checkInTime"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let check_out_time = listing
        .get("checkOut")
        .or_else(|| listing.get("checkOutTime"))
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(ListingDetail {
        id: listing_id.to_string(),
        name,
        location,
        description,
        price_per_night,
        currency,
        rating,
        review_count,
        property_type,
        host_name,
        url: format!("{base_url}/rooms/{listing_id}"),
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
    })
}

fn find_listing_data(data: &serde_json::Value) -> Option<&serde_json::Value> {
    let paths: &[&[&str]] = &[
        &["props", "pageProps", "listing"],
        &["props", "pageProps", "listingData", "listing"],
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

    // Deep search for an object with "name" + ("description" or "amenities") fields
    deep_find_listing(data, 20)
}

fn deep_find_listing(data: &serde_json::Value, max_depth: u32) -> Option<&serde_json::Value> {
    if max_depth == 0 {
        return None;
    }
    match data {
        serde_json::Value::Object(map) => {
            if map.contains_key("name")
                && (map.contains_key("description") || map.contains_key("amenities"))
            {
                return Some(data);
            }
            for value in map.values() {
                if let Some(result) = deep_find_listing(value, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(result) = deep_find_listing(item, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_detail_css(
    document: &Html,
    html: &str,
    listing_id: &str,
    base_url: &str,
) -> Result<ListingDetail> {
    // Structured data was not found: only a page that identifies itself as this
    // listing may be read through CSS; anything else (bot challenge, consent wall,
    // error page served with HTTP 200) is upstream drift, never a fake listing.
    if !page_markers::is_listing_page(document, html, listing_id) {
        return Err(AirbnbError::UpstreamSchema {
            operation: "listing page (HTML)".into(),
            detail: format!(
                "no listing data and no canonical /rooms/{listing_id} marker (bot challenge, consent wall or changed page layout)"
            ),
        });
    }

    let title_selector =
        Selector::parse("h1, [data-testid='listing-title']").map_err(|e| AirbnbError::Parse {
            reason: format!("invalid selector: {e}"),
        })?;

    let name = document.select(&title_selector).next().map_or_else(
        || "Unknown listing".to_string(),
        |el| el.text().collect::<String>().trim().to_string(),
    );

    Ok(ListingDetail {
        id: listing_id.to_string(),
        name,
        location: String::new(),
        description: String::new(),
        price_per_night: 0.0,
        currency: String::new(),
        rating: None,
        review_count: 0,
        property_type: None,
        host_name: None,
        url: format!("{base_url}/rooms/{listing_id}"),
        amenities: Vec::new(),
        house_rules: Vec::new(),
        latitude: None,
        longitude: None,
        photos: Vec::new(),
        bedrooms: None,
        beds: None,
        bathrooms: None,
        max_guests: None,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_next_data_detail() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{
            "name":"Test Villa",
            "description":"A beautiful place",
            "city":"Rome",
            "price":200.0,
            "avgRating":4.9,
            "reviewsCount":55,
            "bedrooms":3,
            "beds":4,
            "bathrooms":2.0,
            "personCapacity":6,
            "amenities":[{"name":"WiFi"},{"name":"Pool"}],
            "photos":[{"pictureUrl":"https://example.com/photo1.jpg"}]
        }}}}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "789", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Test Villa");
        assert_eq!(detail.bedrooms, Some(3));
        assert_eq!(detail.amenities.len(), 2);
    }

    #[test]
    fn css_fallback_extracts_title() {
        let html = r#"<html><head><link rel="canonical" href="https://www.airbnb.com/rooms/999"></head><body><h1>Beach Paradise</h1></body></html>"#;
        let detail = parse_listing_detail(html, "999", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Beach Paradise");
    }

    #[test]
    fn css_fallback_rejects_bot_challenge_page() {
        let html = "<html><body><h1>Please verify you are a human</h1></body></html>";
        let err = parse_listing_detail(html, "999", "https://www.airbnb.com").unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamSchema { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn parse_deferred_state_detail() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"props":{"pageProps":{"listing":{
            "name":"Deferred Villa",
            "description":"Lovely",
            "city":"Milan",
            "price":150.0,
            "amenities":[{"name":"AC"}]
        }}}}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "111", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Deferred Villa");
        assert_eq!(detail.amenities, vec!["AC"]);
    }

    #[test]
    fn detail_all_optional_fields() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{
            "name":"Full Listing",
            "description":"Everything filled",
            "location":"NYC",
            "price":250.0,
            "priceCurrency":"USD",
            "avgRating":4.95,
            "reviewsCount":200,
            "roomType":"Entire home",
            "host":{"name":"Jane"},
            "bedrooms":4,
            "beds":5,
            "bathrooms":3.0,
            "personCapacity":10,
            "checkIn":"14:00",
            "checkOut":"10:00",
            "lat":40.7128,
            "lng":-74.006,
            "amenities":[{"name":"WiFi"},{"name":"Pool"},{"name":"Gym"}],
            "houseRules":["No smoking","No pets"],
            "photos":[{"pictureUrl":"https://example.com/1.jpg"},{"pictureUrl":"https://example.com/2.jpg"}]
        }}}}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "42", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.bedrooms, Some(4));
        assert_eq!(detail.beds, Some(5));
        assert_eq!(detail.bathrooms, Some(3.0));
        assert_eq!(detail.max_guests, Some(10));
        assert_eq!(detail.check_in_time, Some("14:00".into()));
        assert_eq!(detail.check_out_time, Some("10:00".into()));
        assert!((detail.latitude.unwrap() - 40.7128).abs() < 0.001);
        assert!((detail.longitude.unwrap() - (-74.006)).abs() < 0.001);
        assert_eq!(detail.amenities.len(), 3);
        assert_eq!(detail.house_rules.len(), 2);
        assert_eq!(detail.photos.len(), 2);
        assert_eq!(detail.host_name, Some("Jane".into()));
        assert_eq!(detail.property_type, Some("Entire home".into()));
    }

    #[test]
    fn detail_missing_optional_fields() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{
            "name":"Minimal Listing",
            "description":"Just basics"
        }}}}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Minimal Listing");
        assert_eq!(detail.bedrooms, None);
        assert_eq!(detail.beds, None);
        assert_eq!(detail.bathrooms, None);
        assert_eq!(detail.max_guests, None);
        assert_eq!(detail.check_in_time, None);
        assert_eq!(detail.check_out_time, None);
        assert_eq!(detail.latitude, None);
        assert_eq!(detail.longitude, None);
        assert!(detail.amenities.is_empty());
        assert!(detail.house_rules.is_empty());
        assert!(detail.photos.is_empty());
    }

    #[test]
    fn amenities_from_string_array() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{
            "name":"String Amenities",
            "description":"Test",
            "amenities":["WiFi","Pool","Parking"]
        }}}}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "2", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.amenities, vec!["WiFi", "Pool", "Parking"]);
    }

    #[test]
    fn deep_find_listing_nested() {
        let data: serde_json::Value = serde_json::from_str(
            r#"{"wrapper":{"nested":{"name":"Deep Listing","description":"Found deep","amenities":[]}}}"#
        ).unwrap();
        let found = deep_find_listing(&data, 20);
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().get("name").unwrap().as_str().unwrap(),
            "Deep Listing"
        );
    }

    #[test]
    fn parse_niobe_pdp_sections() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"niobeClientData":[["StaysPdpSections:test",{
            "data":{"presentation":{"stayProductDetailPage":{
                "sections":{
                    "metadata":{
                        "sharingConfig":{
                            "title":"Rental unit in Paris · ⭐5.0 · 1 bedroom · 1 bed · 1 shared bath",
                            "propertyType":"Private room in rental unit",
                            "location":"Paris",
                            "personCapacity":2,
                            "imageUrl":"https://example.com/photo.jpg",
                            "reviewCount":10,
                            "starRating":5.0
                        },
                        "loggingContext":{"eventDataLogging":{
                            "listingId":"123",
                            "listingLat":48.85,
                            "listingLng":2.29,
                            "roomType":"Private room"
                        }}
                    },
                    "sections":[
                        {"sectionComponentType":"DESCRIPTION_DEFAULT","section":{
                            "htmlDescription":{"htmlText":"A lovely <b>room</b> in Paris<br />Near metro"}
                        }},
                        {"sectionComponentType":"AMENITIES_DEFAULT","section":{
                            "previewAmenitiesGroups":[
                                {"amenities":[{"title":"Kitchen"},{"title":"Wifi"}]}
                            ]
                        }},
                        {"sectionComponentType":"REVIEWS_DEFAULT","section":{
                            "overallRating":5.0,
                            "overallCount":10,
                            "ratings":[{"label":"Cleanliness","localizedRating":"5.0"}]
                        }},
                        {"sectionComponentType":"LOCATION_PDP","section":{
                            "lat":48.8567,
                            "lng":2.2945,
                            "subtitle":"Paris, France"
                        }},
                        {"sectionComponentType":"POLICIES_DEFAULT","section":{
                            "houseRules":[
                                {"title":"Check-in: 2:00 PM - 11:00 PM"},
                                {"title":"Checkout before 10:00 AM"},
                                {"title":"2 guests maximum"}
                            ]
                        }},
                        {"sectionComponentType":"AVAILABILITY_CALENDAR_DEFAULT","section":{
                            "maxGuestCapacity":2,
                            "listingTitle":"Cozy Room"
                        }}
                    ]
                }
            }}},
            "node":null
        }]]}
        </script></head><body></body></html>"#;

        let detail = parse_listing_detail(html, "123", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Cozy Room");
        assert_eq!(detail.location, "Paris, France");
        assert!(detail.description.contains("lovely"));
        assert!(detail.description.contains("room"));
        assert!(!detail.description.contains("<b>"));
        assert_eq!(detail.rating, Some(5.0));
        assert_eq!(detail.review_count, 10);
        assert_eq!(
            detail.property_type,
            Some("Private room in rental unit".into())
        );
        assert_eq!(detail.amenities, vec!["Kitchen", "Wifi"]);
        assert_eq!(detail.house_rules.len(), 3);
        assert!((detail.latitude.unwrap() - 48.8567).abs() < 0.001);
        assert!((detail.longitude.unwrap() - 2.2945).abs() < 0.001);
        assert_eq!(detail.max_guests, Some(2));
        assert!(detail.check_in_time.is_some());
        assert!(detail.check_out_time.is_some());
        assert_eq!(detail.bedrooms, Some(1));
        assert_eq!(detail.beds, Some(1));
        assert_eq!(detail.bathrooms, Some(1.0));
    }

    #[test]
    fn legacy_detail_currency_is_normalized_or_left_unknown() {
        let with_iso = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{"name":"Iso","description":"x","price":90.0,"priceCurrency":"EUR"}}}}
        </script></head><body></body></html>"#;
        assert_eq!(
            parse_listing_detail(with_iso, "1", "https://www.airbnb.com")
                .unwrap()
                .currency,
            "\u{20ac}"
        );
        let without = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"listing":{"name":"None","description":"x","price":90.0}}}}
        </script></head><body></body></html>"#;
        assert_eq!(
            parse_listing_detail(without, "1", "https://www.airbnb.com")
                .unwrap()
                .currency,
            ""
        );
    }

    use crate::test_helpers::{fixture_json, niobe_page};

    fn pdp_page(wrapper: &str, payload: &serde_json::Value) -> String {
        niobe_page(wrapper, &[("StaysPdpSections:{}", payload)])
    }

    #[test]
    fn scraper_pdp_detail_keeps_every_amenity_and_photo() {
        let html = pdp_page("niobeClientData", &fixture_json("p1b/pdp_apartment.json"));
        let detail = parse_listing_detail(&html, "38817969", "https://www.airbnb.com").unwrap();
        assert_eq!(
            detail.amenities,
            vec!["Hair dryer", "Shampoo", "Hot water", "Smoke alarm"]
        );
        assert!(
            !detail
                .amenities
                .iter()
                .any(|a| a == "Carbon monoxide alarm")
        );
        assert_eq!(detail.photos.len(), 4);
        assert_eq!(detail.name, "Charming apartment - Lyon center");
        assert_eq!(detail.host_id.as_deref(), Some("1000001"));
        assert_eq!(detail.location, "Lyon, Auvergne-Rh\u{f4}ne-Alpes, France");
    }

    #[test]
    fn scraper_pdp_price_keeps_the_currency_it_was_printed_with() {
        let mut payload = fixture_json("p1b/pdp_apartment.json");
        let sections = payload
            .pointer_mut("/data/presentation/stayProductDetailPage/sections/sections")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap();
        let book_it = sections
            .iter_mut()
            .find(|s| s["sectionComponentType"] == "BOOK_IT_SIDEBAR")
            .unwrap();
        book_it["section"]["structuredDisplayPrice"] =
            serde_json::json!({"primaryLine": {"price": "\u{20ac}120", "qualifier": "night"}});
        let detail = parse_listing_detail(
            &pdp_page("niobeClientData", &payload),
            "38817969",
            "https://www.airbnb.com",
        )
        .unwrap();
        assert_eq!(detail.known_price(), Some(120.0));
        assert_eq!(detail.currency, "\u{20ac}");
    }

    #[test]
    fn pdp_without_metadata_is_parsed_from_its_sections() {
        let mut payload = fixture_json("p1b/pdp_apartment.json");
        payload
            .pointer_mut("/data/presentation/stayProductDetailPage/sections")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap()
            .remove("metadata");
        let detail = parse_listing_detail(
            &pdp_page("niobeClientData", &payload),
            "38817969",
            "https://www.airbnb.com",
        )
        .unwrap();
        assert_eq!(detail.name, "Charming apartment - Lyon center");
        assert_eq!(detail.amenities.len(), 4);
    }

    #[test]
    fn unrelated_entry_before_the_pdp_does_not_win() {
        let promo =
            serde_json::json!({"data": {"x": {"name": "Promo", "description": "Save 10%"}}});
        let payload = fixture_json("p1b/pdp_apartment.json");
        let html = niobe_page(
            "niobeClientData",
            &[("Other:{}", &promo), ("StaysPdpSections:{}", &payload)],
        );
        let detail = parse_listing_detail(&html, "38817969", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Charming apartment - Lyon center");
    }

    #[test]
    fn niobe_minimal_wrapper_is_unwrapped_for_detail_and_host() {
        let html = pdp_page(
            "niobeMinimalClientData",
            &fixture_json("p1b/pdp_apartment.json"),
        );
        let page = parse_listing_page(&html, "38817969", "https://www.airbnb.com");
        assert_eq!(
            page.detail.unwrap().name,
            "Charming apartment - Lyon center"
        );
        assert_eq!(page.host.unwrap().name, "Host A");
    }

    #[test]
    fn hotel_page_host_is_reported_as_unavailable() {
        let html = pdp_page("niobeClientData", &fixture_json("p1b/pdp_hotel.json"));
        let err = parse_host_profile(&html).unwrap_err();
        assert!(
            matches!(err, AirbnbError::HostProfileUnavailable { .. }),
            "{err}"
        );
    }

    #[test]
    fn numeric_host_id_in_logging_is_kept_as_digits() {
        let mut payload = fixture_json("p1b/pdp_hotel.json");
        payload
            .pointer_mut("/data/presentation/stayProductDetailPage/sections/metadata/loggingContext/eventDataLogging")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap()
            .insert("hostId".into(), serde_json::json!(12345));
        let detail = parse_listing_detail(
            &pdp_page("niobeClientData", &payload),
            "1257736932578886647",
            "https://www.airbnb.com",
        )
        .unwrap();
        assert_eq!(detail.host_id.as_deref(), Some("12345"));
    }
}
