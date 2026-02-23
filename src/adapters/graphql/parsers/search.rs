use serde_json::Value;

use crate::domain::listing::{Listing, SearchResult};
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};

/// Build GraphQL variables for the `StaysSearch` operation.
pub fn build_search_variables(params: &SearchParams) -> Value {
    let mut raw_params = vec![
        serde_json::json!({"filterName": "cdnCacheSafe", "filterValues": ["false"]}),
        serde_json::json!({"filterName": "channel", "filterValues": ["EXPLORE"]}),
        serde_json::json!({"filterName": "placeId", "filterValues": [params.location]}),
        serde_json::json!({"filterName": "source", "filterValues": ["structured_search_input_header"]}),
        serde_json::json!({"filterName": "searchType", "filterValues": ["filter_change"]}),
    ];

    if let Some(ref checkin) = params.checkin {
        raw_params.push(serde_json::json!({"filterName": "checkin", "filterValues": [checkin]}));
    }
    if let Some(ref checkout) = params.checkout {
        raw_params.push(serde_json::json!({"filterName": "checkout", "filterValues": [checkout]}));
    }
    if let Some(adults) = params.adults {
        raw_params.push(
            serde_json::json!({"filterName": "adults", "filterValues": [adults.to_string()]}),
        );
    }
    if let Some(children) = params.children {
        raw_params.push(
            serde_json::json!({"filterName": "children", "filterValues": [children.to_string()]}),
        );
    }
    if let Some(infants) = params.infants {
        raw_params.push(
            serde_json::json!({"filterName": "infants", "filterValues": [infants.to_string()]}),
        );
    }
    if let Some(pets) = params.pets {
        raw_params
            .push(serde_json::json!({"filterName": "pets", "filterValues": [pets.to_string()]}));
    }
    if let Some(min_price) = params.min_price {
        raw_params.push(
            serde_json::json!({"filterName": "priceMin", "filterValues": [min_price.to_string()]}),
        );
    }
    if let Some(max_price) = params.max_price {
        raw_params.push(
            serde_json::json!({"filterName": "priceMax", "filterValues": [max_price.to_string()]}),
        );
    }

    serde_json::json!({
        "staysSearchRequest": {
            "requestedPageType": "STAYS_SEARCH",
            "metadataOnly": false,
            "searchType": "filter_change",
            "treatmentFlags": ["decompose_stays_search_m2_treatment"],
            "rawParams": raw_params,
        },
        "staysMapSearchRequestV2": {
            "requestedPageType": "STAYS_SEARCH",
            "metadataOnly": false,
            "searchType": "filter_change",
            "treatmentFlags": ["decompose_stays_search_m2_treatment"],
            "rawParams": raw_params,
        }
    })
}

/// Parse the GraphQL `StaysSearch` response into a `SearchResult`.
#[allow(clippy::too_many_lines)]
pub fn parse_search_response(json: &Value, base_url: &str) -> Result<SearchResult> {
    // Try multiple response paths — Airbnb's structure can vary
    let results = json
        .pointer("/data/presentation/staysSearch/results/searchResults")
        .or_else(|| json.pointer("/data/presentation/explore/sections/sectionIndependentData/staysSearch/searchResults"))
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::Parse {
            reason: "GraphQL search: could not find searchResults array".into(),
        })?;

    let mut listings = Vec::new();

    for result in results {
        let listing_data = result.pointer("/listing").unwrap_or(result);

        let id = listing_data
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        if id.is_empty() {
            continue;
        }

        let name = listing_data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string();

        let location = listing_data
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        // Price extraction — nested pricing structure
        let price_per_night = result
            .pointer("/pricingQuote/structuredStayDisplayPrice/primaryLine/price")
            .and_then(Value::as_str)
            .and_then(extract_price_number)
            .or_else(|| {
                result
                    .pointer("/pricingQuote/rate/amount")
                    .and_then(Value::as_f64)
            })
            .unwrap_or(0.0);

        let currency = result
            .pointer("/pricingQuote/rate/currency")
            .and_then(Value::as_str)
            .unwrap_or("USD")
            .to_string();

        let rating = listing_data.get("avgRating").and_then(Value::as_f64);

        let review_count = listing_data
            .get("reviewsCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        let thumbnail_url = listing_data
            .pointer("/contextualPictures/0/picture")
            .and_then(Value::as_str)
            .map(String::from);

        let property_type = listing_data
            .get("roomTypeCategory")
            .and_then(Value::as_str)
            .map(String::from);

        let is_superhost = listing_data.get("isSuperhost").and_then(Value::as_bool);

        let lat = listing_data
            .get("latitude")
            .or_else(|| listing_data.pointer("/coordinate/latitude"))
            .and_then(Value::as_f64);

        let lng = listing_data
            .get("longitude")
            .or_else(|| listing_data.pointer("/coordinate/longitude"))
            .and_then(Value::as_f64);

        let total_price = result
            .pointer("/pricingQuote/structuredStayDisplayPrice/primaryLine/originalPrice")
            .and_then(Value::as_str)
            .and_then(extract_price_number);

        let url = format!("{base_url}/rooms/{id}");

        listings.push(Listing {
            id,
            name,
            location,
            price_per_night,
            currency,
            rating,
            review_count,
            thumbnail_url,
            property_type,
            host_name: None,
            url,
            is_superhost,
            is_guest_favorite: None,
            instant_book: None,
            total_price,
            photos: Vec::new(),
            latitude: lat,
            longitude: lng,
        });
    }

    let total_count = json
        .pointer("/data/presentation/staysSearch/results/paginationInfo/totalCount")
        .and_then(Value::as_u64)
        .map(|n| n as u32);

    let next_cursor = json
        .pointer("/data/presentation/staysSearch/results/paginationInfo/nextPageCursor")
        .and_then(Value::as_str)
        .map(String::from);

    Ok(SearchResult {
        listings,
        total_count,
        next_cursor,
    })
}

/// Extract a numeric price from strings like "$120", "€95", "120 USD".
fn extract_price_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cleaned.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_price_from_dollar_string() {
        assert!((extract_price_number("$120").unwrap() - 120.0).abs() < 0.01);
    }

    #[test]
    fn extract_price_from_euro_string() {
        assert!((extract_price_number("€95.50").unwrap() - 95.50).abs() < 0.01);
    }

    #[test]
    fn extract_price_empty() {
        assert!(extract_price_number("").is_none());
    }

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
    fn extract_price_no_numeric_chars() {
        assert!(extract_price_number("free").is_none());
        assert!(extract_price_number("Contact host").is_none());
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
}
