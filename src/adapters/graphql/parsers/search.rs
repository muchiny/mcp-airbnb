use serde_json::Value;

use crate::adapters::price;
use crate::adapters::stay_search::listing_from_stay_search_result;
use crate::adapters::text;
use crate::domain::listing::{Listing, SearchResult};
use crate::domain::search_params::{
    SEARCH_PAGE_SIZE, SUPPORTED_PROPERTY_TYPES, SearchParams, canonical_property_type,
};
use crate::error::{AirbnbError, Result};

/// Treatment flags sent by the airbnb.com web client with `StaysSearch` (captured 2026-09).
const SEARCH_TREATMENT_FLAGS: [&str; 11] = [
    "feed_map_decouple_m11_treatment",
    "recommended_amenities_2024_treatment_b",
    "filter_redesign_2024_treatment",
    "filter_reordering_2024_roomtype_treatment",
    "p2_category_bar_removal_treatment",
    "selected_filters_2024_treatment",
    "recommended_filters_2024_treatment_b",
    "m13_search_input_phase2_treatment",
    "m13_search_input_services_enabled",
    "m13_2025_experiences_p2_treatment",
    "homes_p25_refresh_2025_treatment",
];

/// `version` raw param sent by the airbnb.com web client (captured 2026-09).
const SEARCH_CLIENT_VERSION: &str = "1.8.8";

/// `rawParams` filter for a supported property type. The values are those of the
/// "Type of place" options in the captured 2026-09 filter panel (`room_types`,
/// `kg_and_tags` URL params); raw param names are the camelCase form of URL
/// params, as in the captured request (`price_filter_input_type` → `priceFilterInputType`).
fn property_type_raw_param(property_type: &str) -> Option<(&'static str, &'static str)> {
    match canonical_property_type(property_type)? {
        "Entire home" => Some(("roomTypes", "Entire home/apt")),
        "Private room" => Some(("roomTypes", "Private room")),
        "Hotel room" => Some(("kgAndTags", "Tag:9613")),
        _ => None,
    }
}

/// Build GraphQL variables for the `StaysSearch` operation, mirroring the
/// airbnb.com web client (2026-09): the location travels as the `query` raw
/// param (no Google `placeId`), filters as sorted `rawParams`, and the page
/// cursor as `cursor` on both the list and the map request.
pub fn build_search_variables(params: &SearchParams) -> Result<Value> {
    let mut filters: Vec<(&'static str, String)> = vec![
        ("cdnCacheSafe", "false".to_string()),
        ("channel", "EXPLORE".to_string()),
        ("itemsPerGrid", SEARCH_PAGE_SIZE.to_string()),
        ("query", params.location.trim().to_string()),
        ("refinementPaths", "/homes".to_string()),
        ("screenSize", "large".to_string()),
        ("tabId", "home_tab".to_string()),
        ("version", SEARCH_CLIENT_VERSION.to_string()),
    ];
    if let Some(ref checkin) = params.checkin {
        filters.push(("checkin", checkin.clone()));
    }
    if let Some(ref checkout) = params.checkout {
        filters.push(("checkout", checkout.clone()));
    }
    if let Some(adults) = params.adults {
        filters.push(("adults", adults.to_string()));
    }
    if let Some(children) = params.children {
        filters.push(("children", children.to_string()));
    }
    if let Some(infants) = params.infants {
        filters.push(("infants", infants.to_string()));
    }
    if let Some(pets) = params.pets {
        filters.push(("pets", pets.to_string()));
    }
    if let Some(min_price) = params.min_price {
        filters.push(("priceMin", min_price.to_string()));
    }
    if let Some(max_price) = params.max_price {
        filters.push(("priceMax", max_price.to_string()));
    }
    if let Some(ref property_type) = params.property_type {
        let (name, value) =
            property_type_raw_param(property_type).ok_or_else(|| AirbnbError::InvalidParams {
                reason: format!(
                    "unsupported property_type; expected one of: {}",
                    SUPPORTED_PROPERTY_TYPES.join(", ")
                ),
            })?;
        filters.push((name, value.to_string()));
    }
    filters.sort_by_key(|(name, _)| *name);

    // The map request carries the same filters minus the grid size (as captured).
    let raw_params = |include_grid: bool| -> Vec<Value> {
        filters
            .iter()
            .filter(|(name, _)| include_grid || *name != "itemsPerGrid")
            .map(|(name, value)| serde_json::json!({"filterName": name, "filterValues": [value]}))
            .collect()
    };

    let mut list_request = serde_json::json!({
        "metadataOnly": false,
        "requestedPageType": "STAYS_SEARCH",
        "treatmentFlags": SEARCH_TREATMENT_FLAGS,
        "maxMapItems": 9999,
        "rawParams": raw_params(true),
    });
    let mut map_request = serde_json::json!({
        "metadataOnly": false,
        "requestedPageType": "STAYS_SEARCH",
        "treatmentFlags": SEARCH_TREATMENT_FLAGS,
        "rawParams": raw_params(false),
    });
    if let Some(ref cursor) = params.cursor {
        list_request["cursor"] = Value::String(cursor.clone());
        map_request["cursor"] = Value::String(cursor.clone());
    }

    Ok(serde_json::json!({
        "staysSearchRequest": list_request,
        "staysMapSearchRequestV2": map_request,
        "isLeanTreatment": false,
    }))
}

/// Parse a `StaysSearch` response to a **first-page** request (no cursor).
///
/// Kept with its original signature for the fuzz target and single-page callers.
/// Anything that paginates must call [`parse_search_page`] with the cursor it sent.
pub fn parse_search_response(json: &Value, base_url: &str) -> Result<SearchResult> {
    parse_search_page(json, base_url, None)
}

/// Parse the GraphQL `StaysSearch` response into a `SearchResult`.
///
/// Items in the current shape (`StaySearchResult` with `demandStayListing`,
/// 2026-09) go through the extractor shared with the HTML scraper; legacy
/// `listing` items are still accepted. `request_cursor` is the cursor that was
/// sent: the current response only lists every page cursor
/// (`paginationInfo.pageCursors`), so the next page is the entry that follows
/// the requested one.
pub fn parse_search_page(
    json: &Value,
    base_url: &str,
    request_cursor: Option<&str>,
) -> Result<SearchResult> {
    let results_root = json.pointer("/data/presentation/staysSearch/results");
    let results = results_root
        .and_then(|results| results.get("searchResults"))
        .or_else(|| {
            json.pointer(
                "/data/presentation/explore/sections/sectionIndependentData/staysSearch/searchResults",
            )
        })
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::UpstreamSchema {
            operation: "StaysSearch".into(),
            detail: "could not find searchResults array at data.presentation.staysSearch.results"
                .into(),
        })?;

    let listings: Vec<Listing> = results
        .iter()
        .filter_map(|result| {
            if result.get("demandStayListing").is_some() {
                listing_from_stay_search_result(result, base_url)
            } else {
                parse_legacy_result(result, base_url)
            }
        })
        .collect();

    if listings.is_empty() && !results.is_empty() {
        return Err(AirbnbError::UpstreamSchema {
            operation: "StaysSearch".into(),
            detail: format!(
                "{} searchResults item(s) but none had a recognizable listing id (demandStayListing.id or listing.id)",
                results.len()
            ),
        });
    }

    let pagination = results_root.and_then(|results| results.get("paginationInfo"));
    let total_count = pagination
        .and_then(|p| p.get("totalCount"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let next_cursor = next_page_cursor(pagination, request_cursor);

    Ok(SearchResult {
        listings,
        total_count,
        next_cursor,
    })
}

/// Legacy `{ "listing": {...}, "pricingQuote": {...} }` item (older persisted queries).
///
/// The nightly price and its currency come from the same display string
/// (`discountedPrice`, then `price`, read by
/// [`price::parse_structured_display_price`]), then from `pricingQuote.rate`.
/// The struck-through `originalPrice` is never reported as `total_price`.
fn parse_legacy_result(result: &Value, base_url: &str) -> Option<Listing> {
    let listing_data = result.pointer("/listing").unwrap_or(result);
    let id = listing_data
        .get("id")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .filter(|id| !id.is_empty())?;
    let display = result
        .pointer("/pricingQuote/structuredStayDisplayPrice")
        .map(|sdp| price::parse_structured_display_price(sdp, None))
        .unwrap_or_default();
    let price_per_night = display
        .nightly
        .or_else(|| {
            result
                .pointer("/pricingQuote/rate/amount")
                .and_then(Value::as_f64)
                .filter(|p| p.is_finite() && *p > 0.0)
        })
        .unwrap_or(0.0);
    let currency = display
        .currency
        .or_else(|| {
            result
                .pointer("/pricingQuote/rate/currency")
                .and_then(Value::as_str)
                .map(price::normalize_currency)
        })
        .unwrap_or_default();
    Some(Listing {
        url: format!("{base_url}/rooms/{id}"),
        name: listing_data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string(),
        location: listing_data
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        price_per_night,
        currency,
        rating: listing_data.get("avgRating").and_then(Value::as_f64),
        review_count: listing_data
            .get("reviewsCount")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        thumbnail_url: listing_data
            .pointer("/contextualPictures/0/picture")
            .and_then(Value::as_str)
            .map(str::to_string),
        property_type: listing_data
            .get("roomTypeCategory")
            .and_then(Value::as_str)
            .map(str::to_string),
        host_name: None,
        host_id: listing_data
            .pointer("/user/id")
            .or_else(|| listing_data.get("hostId"))
            .and_then(text::user_id_from_json),
        is_superhost: listing_data.get("isSuperhost").and_then(Value::as_bool),
        is_guest_favorite: None,
        instant_book: None,
        // Never `originalPrice`: that is the struck-through pre-discount price.
        total_price: display.total,
        photos: Vec::new(),
        latitude: listing_data
            .get("latitude")
            .or_else(|| listing_data.pointer("/coordinate/latitude"))
            .and_then(Value::as_f64),
        longitude: listing_data
            .get("longitude")
            .or_else(|| listing_data.pointer("/coordinate/longitude"))
            .and_then(Value::as_f64),
        id,
    })
}

/// Cursor of the page after `request_cursor`, or `None` when there is no further
/// page or the requested cursor is not in the list (never loops back to page 1).
fn next_page_cursor(pagination: Option<&Value>, request_cursor: Option<&str>) -> Option<String> {
    let pagination = pagination?;
    if let Some(next) = pagination.get("nextPageCursor").and_then(Value::as_str) {
        return (!next.is_empty() && Some(next) != request_cursor).then(|| next.to_owned());
    }
    let cursors: Vec<&str> = pagination
        .get("pageCursors")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let current = match request_cursor {
        None => 0,
        Some(requested) => cursors.iter().position(|c| *c == requested)?,
    };
    cursors.get(current + 1).copied().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_results() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "staysSearch": {
                        "results": {
                            "searchResults": [],
                            "paginationInfo": {
                                "totalCount": 0
                            }
                        }
                    }
                }
            }
        });
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert!(result.listings.is_empty());
        assert_eq!(result.total_count, Some(0));
    }

    #[test]
    fn parse_listing_without_optional_fields() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "staysSearch": {
                        "results": {
                            "searchResults": [{
                                "listing": {
                                    "id": "999",
                                    "name": "Minimal Place",
                                    "city": "Unknown"
                                },
                                "pricingQuote": {}
                            }],
                            "paginationInfo": {}
                        }
                    }
                }
            }
        });
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        let listing = &result.listings[0];
        assert_eq!(listing.id, "999");
        assert!(listing.rating.is_none());
        assert!(listing.thumbnail_url.is_none());
        assert!(listing.property_type.is_none());
        assert!(listing.latitude.is_none());
        assert!(listing.longitude.is_none());
        assert!((listing.price_per_night - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_listing_skips_empty_id() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "staysSearch": {
                        "results": {
                            "searchResults": [
                                { "listing": { "id": "", "name": "Empty ID" } },
                                { "listing": { "id": "123", "name": "Valid" }, "pricingQuote": {} }
                            ],
                            "paginationInfo": {}
                        }
                    }
                }
            }
        });
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].id, "123");
    }

    #[test]
    fn parse_alternate_response_path() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "explore": {
                        "sections": {
                            "sectionIndependentData": {
                                "staysSearch": {
                                    "searchResults": [{
                                        "listing": {
                                            "id": "alt1",
                                            "name": "Alt Path Listing",
                                            "city": "Berlin"
                                        },
                                        "pricingQuote": {
                                            "rate": { "amount": 85.0, "currency": "EUR" }
                                        }
                                    }]
                                }
                            }
                        }
                    }
                }
            }
        });
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        assert_eq!(result.listings[0].name, "Alt Path Listing");
        assert!((result.listings[0].price_per_night - 85.0).abs() < 0.01);
    }

    #[test]
    fn parse_single_listing() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "staysSearch": {
                        "results": {
                            "searchResults": [{
                                "listing": {
                                    "id": "12345",
                                    "name": "Cozy Apartment",
                                    "city": "Paris",
                                    "avgRating": 4.85,
                                    "reviewsCount": 42,
                                    "isSuperhost": true,
                                    "latitude": 48.8566,
                                    "longitude": 2.3522,
                                },
                                "pricingQuote": {
                                    "rate": {
                                        "amount": 120.0,
                                        "currency": "EUR"
                                    }
                                }
                            }],
                            "paginationInfo": {
                                "totalCount": 1,
                                "nextPageCursor": "page2token"
                            }
                        }
                    }
                }
            }
        });
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings.len(), 1);
        let listing = &result.listings[0];
        assert_eq!(listing.id, "12345");
        assert_eq!(listing.name, "Cozy Apartment");
        assert!((listing.price_per_night - 120.0).abs() < 0.01);
        assert_eq!(listing.is_superhost, Some(true));
        assert_eq!(result.next_cursor, Some("page2token".to_string()));
    }

    use std::collections::HashMap;

    /// `(filterName, filterValues)` pairs of one `rawParams` array.
    fn raw_params(request: &Value) -> Vec<(String, Vec<String>)> {
        request["rawParams"]
            .as_array()
            .expect("rawParams array")
            .iter()
            .map(|param| {
                let name = param["filterName"].as_str().unwrap_or_default().to_string();
                let values = param["filterValues"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                (name, values)
            })
            .collect()
    }

    #[test]
    fn variables_mirror_the_2026_09_web_client_request() {
        let captured = crate::test_helpers::fixture_json("graphql/StaysSearch.request.json");
        let captured_vars = &captured["variables"];
        let params = SearchParams {
            location: "Lyon, France".into(),
            adults: Some(2),
            ..SearchParams::default()
        };
        let ours = build_search_variables(&params).unwrap();

        for request in ["staysSearchRequest", "staysMapSearchRequestV2"] {
            for field in [
                "metadataOnly",
                "requestedPageType",
                "treatmentFlags",
                "maxMapItems",
            ] {
                assert_eq!(
                    ours[request][field], captured_vars[request][field],
                    "{request}.{field}"
                );
            }
            let captured_params: HashMap<String, Vec<String>> =
                raw_params(&captured_vars[request]).into_iter().collect();
            for (name, values) in raw_params(&ours[request]) {
                let captured_values = captured_params
                    .get(&name)
                    .unwrap_or_else(|| panic!("{request}: {name} is not sent by the web client"));
                assert_eq!(&values, captured_values, "{request}: value of {name}");
            }
        }
        assert_eq!(ours["isLeanTreatment"], captured_vars["isLeanTreatment"]);
    }

    #[test]
    fn variables_carry_cursor_filters_and_room_type() {
        let params = SearchParams {
            location: " Paris ".into(),
            checkin: Some("2030-06-01".into()),
            checkout: Some("2030-06-05".into()),
            adults: Some(2),
            children: Some(1),
            infants: Some(1),
            pets: Some(1),
            min_price: Some(50),
            max_price: Some(200),
            property_type: Some("Entire home".into()),
            cursor: Some("CURSOR_2".into()),
        };
        let vars = build_search_variables(&params).unwrap();
        for request in ["staysSearchRequest", "staysMapSearchRequestV2"] {
            assert_eq!(vars[request]["cursor"], "CURSOR_2", "{request}");
            let raw: HashMap<String, Vec<String>> =
                raw_params(&vars[request]).into_iter().collect();
            assert_eq!(raw["query"], ["Paris"]);
            assert_eq!(raw["checkin"], ["2030-06-01"]);
            assert_eq!(raw["checkout"], ["2030-06-05"]);
            assert_eq!(raw["adults"], ["2"]);
            assert_eq!(raw["children"], ["1"]);
            assert_eq!(raw["infants"], ["1"]);
            assert_eq!(raw["pets"], ["1"]);
            assert_eq!(raw["priceMin"], ["50"]);
            assert_eq!(raw["priceMax"], ["200"]);
            assert_eq!(raw["roomTypes"], ["Entire home/apt"]);
            assert!(
                !raw.contains_key("placeId"),
                "{request} must not send placeId"
            );
        }
    }

    #[test]
    fn first_page_has_no_cursor_and_map_request_omits_grid_size() {
        let params = SearchParams {
            location: "Paris".into(),
            ..SearchParams::default()
        };
        let vars = build_search_variables(&params).unwrap();
        assert!(vars["staysSearchRequest"].get("cursor").is_none());
        let list: Vec<String> = raw_params(&vars["staysSearchRequest"])
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let map: Vec<String> = raw_params(&vars["staysMapSearchRequestV2"])
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(list.contains(&"itemsPerGrid".to_string()));
        assert!(!map.contains(&"itemsPerGrid".to_string()));
        let mut sorted = list.clone();
        sorted.sort();
        assert_eq!(
            list, sorted,
            "rawParams are sorted by name, like the web client"
        );
    }

    #[test]
    fn hotel_room_is_sent_as_the_hotel_tag() {
        let params = SearchParams {
            location: "Paris".into(),
            property_type: Some("Hotel".into()),
            ..SearchParams::default()
        };
        let vars = build_search_variables(&params).unwrap();
        let raw: HashMap<String, Vec<String>> = raw_params(&vars["staysSearchRequest"])
            .into_iter()
            .collect();
        assert_eq!(raw["kgAndTags"], ["Tag:9613"]);
        assert!(!raw.contains_key("roomTypes"), "{raw:?}");
    }

    #[test]
    fn variables_reject_unsupported_property_type() {
        for rejected in ["Castle", "Shared room"] {
            let params = SearchParams {
                location: "Paris".into(),
                property_type: Some(rejected.into()),
                ..SearchParams::default()
            };
            let err = build_search_variables(&params).unwrap_err();
            assert!(
                matches!(err, AirbnbError::InvalidParams { .. }),
                "{rejected}: got {err:?}"
            );
        }
    }

    const BASE: &str = "https://www.airbnb.com";

    fn web_capture() -> Value {
        crate::test_helpers::fixture_json("graphql/StaysSearch.response.json")
    }

    fn page_cursors(json: &Value) -> Vec<String> {
        json.pointer("/data/presentation/staysSearch/results/paginationInfo/pageCursors")
            .and_then(Value::as_array)
            .expect("capture lists page cursors")
            .iter()
            .filter_map(|c| c.as_str().map(String::from))
            .collect()
    }

    #[test]
    fn parses_2026_09_web_capture() {
        let json = web_capture();
        let request = crate::test_helpers::fixture_json("graphql/StaysSearch.request.json");
        let request_cursor = request["variables"]["staysSearchRequest"]["cursor"].as_str();
        let result = parse_search_page(&json, BASE, request_cursor).unwrap();

        assert_eq!(result.listings.len(), 18);
        let ids: std::collections::HashSet<&str> =
            result.listings.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids.len(), 18, "listing ids are unique");
        assert!(
            result
                .listings
                .iter()
                .all(|l| l.id.chars().all(|c| c.is_ascii_digit()))
        );
        assert!(
            result
                .listings
                .iter()
                .all(|l| !l.name.is_empty() && l.price_per_night > 0.0)
        );
        let first = &result.listings[0];
        let first_item =
            &json["data"]["presentation"]["staysSearch"]["results"]["searchResults"][0];
        assert_eq!(first.id, "1344016074576611955");
        assert_eq!(
            first.name,
            first_item["subtitle"]
                .as_str()
                .expect("capture has a subtitle")
        );
        assert_eq!(
            first.url,
            "https://www.airbnb.com/rooms/1344016074576611955"
        );
        assert_eq!(first.review_count, 74);
        assert!((first.rating.expect("rating") - 4.95).abs() < 1e-9);
        // The capture is page 2 (items_offset 18): the next page is the third cursor.
        assert_eq!(result.next_cursor, Some(page_cursors(&json)[2].clone()));
    }

    #[test]
    fn first_page_points_to_the_second_page_cursor() {
        let json = web_capture();
        let result = parse_search_page(&json, BASE, None).unwrap();
        assert_eq!(result.next_cursor, Some(page_cursors(&json)[1].clone()));
        // The two-argument wrapper is the same first-page parse.
        let wrapper = parse_search_response(&json, BASE).unwrap();
        assert_eq!(wrapper.next_cursor, result.next_cursor);
        assert_eq!(wrapper.listings.len(), result.listings.len());
    }

    #[test]
    fn unknown_request_cursor_ends_pagination() {
        let result = parse_search_page(&web_capture(), BASE, Some("not-a-known-cursor")).unwrap();
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn last_page_cursor_has_no_next_page() {
        let json = web_capture();
        let cursors = page_cursors(&json);
        let last = cursors.last().expect("at least one cursor");
        let result = parse_search_page(&json, BASE, Some(last)).unwrap();
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn legacy_next_cursor_equal_to_the_request_is_dropped() {
        let json = serde_json::json!({"data": {"presentation": {"staysSearch": {"results": {
            "searchResults": [{ "listing": { "id": "1", "name": "A" } }],
            "paginationInfo": { "nextPageCursor": "same" }
        }}}}});
        let result = parse_search_page(&json, BASE, Some("same")).unwrap();
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn numeric_legacy_ids_are_accepted() {
        let json = serde_json::json!({"data": {"presentation": {"staysSearch": {"results": {
            "searchResults": [{ "listing": { "id": 12345, "name": "Numeric" } }],
            "paginationInfo": {}
        }}}}});
        let result = parse_search_page(&json, BASE, None).unwrap();
        assert_eq!(result.listings[0].id, "12345");
    }

    #[test]
    fn unrecognised_items_are_upstream_schema_not_an_empty_success() {
        let json = serde_json::json!({"data": {"presentation": {"staysSearch": {"results": {
            "searchResults": [{ "unexpected": 1 }, { "alsoUnexpected": 2 }],
            "paginationInfo": {}
        }}}}});
        let err = parse_search_page(&json, BASE, None).unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamSchema { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn missing_results_is_upstream_schema() {
        let json = serde_json::json!({"data": {"presentation": null}});
        let err = parse_search_page(&json, BASE, None).unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamSchema { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn stay_search_results_use_the_nightly_price_and_the_real_total() {
        let json = crate::test_helpers::fixture_json("p1b/stays_search.json");
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        let summary: Vec<(&str, Option<f64>, Option<f64>, &str)> = result
            .listings
            .iter()
            .map(|l| {
                (
                    l.id.as_str(),
                    l.known_price(),
                    l.total_price,
                    l.currency.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("1344016074576611955", Some(446.94), Some(2245.0), "$"),
                ("968734101213290865", Some(158.39), Some(797.0), "$"),
                ("1532589801839552925", Some(147.24), Some(810.0), "$"),
            ]
        );
        assert_eq!(result.listings[2].name, "Example Hotel Lyon");
    }

    #[test]
    fn legacy_discounted_line_is_nightly_and_never_a_total() {
        let json = serde_json::json!({"data": {"presentation": {"staysSearch": {"results": {"searchResults": [
            {"listing": {"id": "1"}, "pricingQuote": {"structuredStayDisplayPrice": {"primaryLine": {
                "discountedPrice": "$90", "originalPrice": "$120"
            }}}}
        ]}}}}});
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings[0].known_price(), Some(90.0));
        assert_eq!(result.listings[0].total_price, None);
        assert_eq!(result.listings[0].currency, "$");
    }

    #[test]
    fn legacy_rate_currency_is_normalized_not_defaulted() {
        let json = serde_json::json!({"data": {"presentation": {"staysSearch": {"results": {"searchResults": [
            {"listing": {"id": "2"}, "pricingQuote": {"rate": {"amount": 85.0, "currency": "EUR"}}},
            {"listing": {"id": 3}, "pricingQuote": {}}
        ]}}}}});
        let result = parse_search_response(&json, "https://www.airbnb.com").unwrap();
        assert_eq!(result.listings[0].currency, "\u{20ac}");
        assert_eq!(result.listings[1].id, "3");
        assert_eq!(result.listings[1].currency, "");
        assert_eq!(result.listings[1].known_price(), None);
    }
}
