use serde_json::Value;
use tracing::debug;

use super::host;
use super::pdp::{self, str_field};
use crate::adapters::price;
use crate::adapters::text;
use crate::domain::listing::ListingDetail;
use crate::error::{AirbnbError, Result};

/// Parse the GraphQL `StaysPdpSections` response into a `ListingDetail`.
///
/// The listing page embeds the same document (`niobeClientData`), so the HTML
/// scraper uses this parser too. Values Airbnb does not publish stay
/// explicitly unknown: `price_per_night` is `0.0` (see
/// [`ListingDetail::known_price`]), `currency` is empty until the client
/// labels it with the pinned currency, and optional fields are `None`. The
/// location is a place name, never a section heading such as "Where you'll be".
#[allow(clippy::too_many_lines)]
pub fn parse_detail_response(json: &Value, id: &str, base_url: &str) -> Result<ListingDetail> {
    let sections = json
        .pointer(pdp::SECTIONS)
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::UpstreamSchema {
            operation: "StaysPdpSections".into(),
            detail: "could not find sections array".into(),
        })?;

    let mut name: Option<String> = None;
    let mut title_subtitle: Option<String> = None;
    let mut location_subtitle: Option<String> = None;
    let mut calendar_location: Option<String> = None;
    let mut calendar_title: Option<String> = None;
    let mut calendar_max_guests: Option<u32> = None;
    let mut calendar_items: Vec<String> = Vec::new();
    let mut legacy_overview_items: Vec<String> = Vec::new();
    let mut description = String::new();
    let mut sidebar_price: Option<(f64, Option<String>)> = None;
    let mut book_it_max_guests: Option<u32> = None;
    let mut rating: Option<f64> = None;
    let mut review_count: Option<u32> = None;
    let mut property_type: Option<String> = None;
    let mut amenities: Vec<String> = Vec::new();
    let mut house_rules: Vec<String> = Vec::new();
    let mut photos: Vec<String> = Vec::new();
    let mut cancellation_policy: Option<String> = None;
    let mut latitude: Option<f64> = None;
    let mut longitude: Option<f64> = None;
    let mut neighborhood: Option<String> = None;

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
                name = name.or_else(|| str_field(Some(data), "title"));
                title_subtitle = title_subtitle.or_else(|| str_field(Some(data), "subtitle"));
            }
            "HERO_DEFAULT" => {
                for url in data
                    .get("previewImages")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|image| image.get("baseUrl").and_then(Value::as_str))
                {
                    push_unique(&mut photos, url);
                }
            }
            "PHOTO_TOUR_SCROLLABLE" | "PHOTO_TOUR_MODAL" => {
                for url in data
                    .get("mediaItems")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| {
                        item.get("baseUrl")
                            .or_else(|| item.get("url"))
                            .and_then(Value::as_str)
                    })
                {
                    push_unique(&mut photos, url);
                }
            }
            "DESCRIPTION_DEFAULT" | "DESCRIPTION_SECTION" => {
                if let Some(html) = data
                    .pointer("/htmlDescription/htmlText")
                    .and_then(Value::as_str)
                {
                    description = text::html_to_text(html);
                } else if let Some(plain) = data.get("description").and_then(Value::as_str) {
                    description = plain.to_string();
                }
            }
            "AMENITIES_DEFAULT" | "AMENITIES_SECTION" => {
                // `seeAllAmenitiesGroups` is the complete list; the preview is a subset.
                let groups = data
                    .get("seeAllAmenitiesGroups")
                    .or_else(|| data.get("previewAmenitiesGroups"))
                    .or_else(|| data.get("amenityGroups"))
                    .and_then(Value::as_array);
                for item in groups
                    .into_iter()
                    .flatten()
                    .filter_map(|group| group.get("amenities").and_then(Value::as_array))
                    .flatten()
                {
                    // Struck-through items ("Not included") are amenities the listing lacks.
                    if item.get("available").and_then(Value::as_bool) == Some(false) {
                        continue;
                    }
                    if let Some(title) = item.get("title").and_then(Value::as_str) {
                        push_unique(&mut amenities, title);
                    }
                }
            }
            "POLICIES_DEFAULT" | "HOUSE_RULES_DEFAULT" => {
                house_rules.extend(item_titles(data.get("houseRules")));
                cancellation_policy = cancellation_policy.or_else(|| {
                    data.pointer("/cancellationPolicy/title")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            }
            "BOOK_IT_SIDEBAR" => {
                sidebar_price = sidebar_price.or_else(|| book_it_price(data));
                book_it_max_guests =
                    book_it_max_guests.or_else(|| u32_value(data.get("maxGuestCapacity")));
            }
            "SBUI_SENTINEL" | "OVERVIEW_DEFAULT"
                if section_type == "OVERVIEW_DEFAULT"
                    || section_id.starts_with("OVERVIEW_DEFAULT") =>
            {
                legacy_overview_items.extend(item_titles(data.get("detailItems")));
            }
            "LOCATION_DEFAULT" | "LOCATION_PDP" => {
                // `title` is the section heading ("Where you'll be"), never a place.
                location_subtitle = location_subtitle.or_else(|| str_field(Some(data), "subtitle"));
                neighborhood = neighborhood.or_else(|| str_field(Some(data), "subtitle"));
                latitude = latitude.or_else(|| data.get("lat").and_then(Value::as_f64));
                longitude = longitude.or_else(|| data.get("lng").and_then(Value::as_f64));
            }
            "AVAILABILITY_CALENDAR_DEFAULT" | "STAYS_PDP_AVAILABILITY_CALENDAR_INLINE" => {
                calendar_location =
                    calendar_location.or_else(|| str_field(Some(data), "localizedLocation"));
                calendar_title = calendar_title.or_else(|| str_field(Some(data), "listingTitle"));
                calendar_max_guests =
                    calendar_max_guests.or_else(|| u32_value(data.get("maxGuestCapacity")));
                if calendar_items.is_empty() {
                    calendar_items = item_titles(data.get("descriptionItems"));
                }
            }
            "REVIEWS_DEFAULT" => {
                rating = rating.or_else(|| data.get("overallRating").and_then(Value::as_f64));
                review_count = review_count.or_else(|| {
                    u32_value(
                        data.get("overallCount")
                            .or_else(|| data.get("reviewsCount")),
                    )
                });
            }
            // Host fields are read once, after the loop, from the best host section.
            "MEET_YOUR_HOST" | "HOST_PROFILE_DEFAULT" | "HOST_OVERVIEW_DEFAULT" => {}
            _ => {
                rating = rating.or_else(|| {
                    data.get("overallRating")
                        .or_else(|| data.pointer("/reviewSummary/overallRating"))
                        .and_then(Value::as_f64)
                });
                review_count = review_count.or_else(|| {
                    u32_value(
                        data.get("overallCount")
                            .or_else(|| data.get("reviewsCount"))
                            .or_else(|| data.pointer("/reviewSummary/totalReviews")),
                    )
                });
                property_type = property_type
                    .or_else(|| str_field(Some(data), "propertyType"))
                    .or_else(|| str_field(Some(data), "roomType"));
            }
        }
    }

    let metadata = json.pointer(pdp::METADATA);
    let sharing = metadata.and_then(|m| m.get("sharingConfig"));
    let logging = metadata.and_then(|m| m.pointer("/loggingContext/eventDataLogging"));
    let booking = metadata.and_then(|m| m.get("bookingPrefetchData"));
    let overview = pdp::sbui_section_data(json, "OVERVIEW_DEFAULT_V2");
    let overview_line = overview
        .and_then(|o| o.get("title"))
        .and_then(Value::as_str)
        .and_then(text::split_type_and_place);

    // A place name from the most precise source; never a section heading.
    let location = title_subtitle
        .or(location_subtitle)
        .or_else(|| overview_line.as_ref().map(|(_, place)| place.clone()))
        .or_else(|| str_field(sharing, "location"))
        .or(calendar_location)
        .unwrap_or_default();
    let name = name
        .or(calendar_title)
        .or_else(|| str_field(sharing, "title"))
        .unwrap_or_default();
    let property_type = property_type
        .or_else(|| str_field(sharing, "propertyType"))
        .or_else(|| overview_line.as_ref().map(|(kind, _)| kind.clone()))
        .or_else(|| str_field(logging, "roomType"));

    let overview_items = item_titles(overview.and_then(|o| o.get("overviewItems")));
    let sharing_parts: Vec<String> = str_field(sharing, "title")
        .map(|title| {
            title
                .split('\u{b7}')
                .map(|part| part.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    let rooms = text::parse_room_counts(overview_items.iter().map(String::as_str))
        .or_fill(text::parse_room_counts(
            legacy_overview_items.iter().map(String::as_str),
        ))
        .or_fill(text::parse_room_counts(
            sharing_parts.iter().map(String::as_str),
        ))
        .or_fill(text::parse_room_counts(
            calendar_items.iter().map(String::as_str),
        ));
    let max_guests = book_it_max_guests
        .or(rooms.guests)
        .or(calendar_max_guests)
        .or_else(|| u32_value(sharing.and_then(|s| s.get("personCapacity"))))
        .or_else(|| u32_value(logging.and_then(|l| l.get("personCapacity"))));

    let rating = rating
        .or_else(|| {
            sharing
                .and_then(|s| s.get("starRating"))
                .and_then(Value::as_f64)
        })
        .or_else(|| {
            logging
                .and_then(|l| l.get("guestSatisfactionOverall"))
                .and_then(Value::as_f64)
        });
    let review_count = review_count
        .or_else(|| u32_value(sharing.and_then(|s| s.get("reviewCount"))))
        .unwrap_or(0);
    let latitude = latitude.or_else(|| {
        logging
            .and_then(|l| l.get("listingLat"))
            .and_then(Value::as_f64)
    });
    let longitude = longitude.or_else(|| {
        logging
            .and_then(|l| l.get("listingLng"))
            .and_then(Value::as_f64)
    });

    let sidebar_price = sidebar_price.or_else(|| logging_price(logging));
    if sidebar_price.is_none() {
        // Expected for detail fetches without dates: Airbnb omits the price.
        debug!(id, "No nightly price in the StaysPdpSections response");
    }
    let (price_per_night, price_currency) = sidebar_price.unwrap_or((0.0, None));
    let currency = price_currency
        .or_else(|| {
            logging
                .and_then(|l| l.get("currency"))
                .and_then(Value::as_str)
                .map(price::normalize_currency)
        })
        .unwrap_or_default();
    let (cleaning_fee, service_fee) = fees(booking);

    let check_in_time = str_field(booking, "checkIn")
        .or_else(|| house_rule_starting_with(&house_rules, &["check-in", "checkin"]));
    let check_out_time = str_field(booking, "checkOut").or_else(|| {
        house_rule_starting_with(&house_rules, &["checkout", "check out", "check-out"])
    });
    let cancellation_policy = cancellation_policy.or_else(|| {
        booking
            .and_then(|b| b.get("cancellationPolicies"))
            .and_then(Value::as_array)?
            .iter()
            .find_map(|policy| str_field(Some(policy), "localized_cancellation_policy_name"))
    });
    let instant_book = logging
        .and_then(|l| l.get("instantBook").or_else(|| l.get("isInstantBook")))
        .and_then(Value::as_bool);

    let host = host::find_host_section(sections)
        .and_then(|section| host::host_profile_from_section(section, host::sbui_host_id(json)));
    let host_ref = host.as_ref();
    let host_id = host_ref
        .and_then(|h| h.host_id.clone())
        .or_else(|| host::sbui_host_id(json))
        .or_else(|| {
            logging
                .and_then(|l| l.get("hostId"))
                .and_then(text::user_id_from_json)
        });

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
        host_name: host_ref.map(|h| h.name.clone()),
        url: format!("{base_url}/rooms/{id}"),
        amenities,
        house_rules,
        latitude,
        longitude,
        photos,
        bedrooms: rooms.bedrooms,
        beds: rooms.beds,
        bathrooms: rooms.bathrooms,
        max_guests,
        check_in_time,
        check_out_time,
        host_id,
        host_is_superhost: host_ref.and_then(|h| h.is_superhost),
        host_response_rate: host_ref.and_then(|h| h.response_rate.clone()),
        host_response_time: host_ref.and_then(|h| h.response_time.clone()),
        host_joined: host_ref.and_then(|h| h.member_since.clone()),
        host_total_listings: host_ref.and_then(|h| h.total_listings),
        host_languages: host_ref.map(|h| h.languages.clone()).unwrap_or_default(),
        cancellation_policy,
        instant_book,
        cleaning_fee,
        service_fee,
        neighborhood,
    })
}

/// Nightly price shown in the booking sidebar, with the currency label
/// printed next to it. Structured prices win over free-text copies.
fn book_it_price(data: &Value) -> Option<(f64, Option<String>)> {
    if let Some(sdp) = data
        .get("structuredDisplayPrice")
        .or_else(|| data.get("structuredStayDisplayPrice"))
    {
        let display = price::parse_structured_display_price(sdp, None);
        if let Some(nightly) = display.nightly {
            return Some((nightly, display.currency));
        }
    }
    let rate_plan = data.get("ratePlanTitle").and_then(Value::as_str);
    let description_titles = data
        .get("descriptionItems")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("title").and_then(Value::as_str));
    let disclaimer = data.get("priceDisclaimer").and_then(Value::as_str);
    rate_plan
        .into_iter()
        .chain(description_titles)
        .chain(disclaimer)
        .find_map(price::find_price_in_text)
        .or_else(|| {
            data.pointer("/price/amount")
                .and_then(Value::as_f64)
                .filter(|p| p.is_finite() && *p > 0.0)
                .map(|p| (p, None))
        })
}

/// Price published in the logging metadata (no currency label).
fn logging_price(logging: Option<&Value>) -> Option<(f64, Option<String>)> {
    let logging = logging?;
    [
        "listingPrice",
        "nightly_price",
        "price",
        "pricePerNight",
        "native_price",
    ]
    .iter()
    .find_map(|key| {
        let value = logging.get(*key)?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(price::parse_price_amount))
            .filter(|p| p.is_finite() && *p > 0.0)
    })
    .map(|p| (p, None))
}

/// Cleaning and service fees from `bookingPrefetchData.priceBreakdown`.
fn fees(booking: Option<&Value>) -> (Option<f64>, Option<f64>) {
    let mut cleaning = None;
    let mut service = None;
    let items = booking
        .and_then(|b| b.pointer("/priceBreakdown/priceItems"))
        .and_then(Value::as_array);
    for item in items.into_iter().flatten() {
        let label = item
            .get("localizedTitle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let amount = item
            .pointer("/total/amountMicros")
            .and_then(Value::as_f64)
            .map(|micros| micros / 1_000_000.0)
            .or_else(|| item.pointer("/total/amount").and_then(Value::as_f64));
        if label.contains("cleaning") {
            cleaning = amount;
        } else if label.contains("service") {
            service = amount;
        }
    }
    (cleaning, service)
}

/// A count published as a number or a numeric string.
fn u32_value(value: Option<&Value>) -> Option<u32> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .and_then(|n| u32::try_from(n).ok())
}

/// `title` of every item of a list section (`detailItems`, `houseRules`, ...).
fn item_titles(items: Option<&Value>) -> Vec<String> {
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("title").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|existing| existing == value) {
        list.push(value.to_string());
    }
}

fn house_rule_starting_with(rules: &[String], prefixes: &[&str]) -> Option<String> {
    rules
        .iter()
        .find(|rule| {
            let lower = rule.to_lowercase();
            prefixes.iter().any(|prefix| lower.starts_with(prefix))
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(detail.host_response_rate, Some("98%".into()));
        assert_eq!(detail.host_response_time, Some("within an hour".into()));
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

    use crate::test_helpers::fixture_json;

    fn sections_json(sections: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"sections": {"sections": sections}}}}})
    }

    #[test]
    fn hotel_fixture_location_is_a_place_not_the_section_heading() {
        let detail = parse_detail_response(
            &fixture_json("p1b/pdp_hotel.json"),
            "1257736932578886647",
            "https://www.airbnb.com",
        )
        .unwrap();
        assert_eq!(detail.location, "V\u{e9}nissieux, France");
        assert_ne!(detail.location, "Where you\u{2019}ll be");
        assert_eq!(detail.name, "Hotel Example Lyon Sud");
        assert_eq!(detail.property_type.as_deref(), Some("Room in hotel"));
        assert_eq!(detail.known_price(), None);
        assert_eq!(detail.currency, "");
        assert_eq!((detail.rating, detail.review_count), (Some(4.4), 359));
        assert_eq!(detail.max_guests, Some(2));
        assert_eq!(
            (detail.bedrooms, detail.beds, detail.bathrooms),
            (None, Some(1), Some(1.0))
        );
        assert_eq!(detail.host_name, None);
        assert_eq!(
            detail.amenities,
            vec![
                "Shampoo",
                "Conditioner",
                "Hot water",
                "Shower gel",
                "Smoke alarm"
            ]
        );
        assert_eq!(detail.photos.len(), 4);
        assert_eq!(
            detail.check_in_time.as_deref(),
            Some("Check-in after 3:00\u{202f}PM")
        );
        assert_eq!(
            detail.check_out_time.as_deref(),
            Some("Checkout before 11:00\u{202f}AM")
        );
        assert_eq!(detail.cancellation_policy.as_deref(), Some("Flexible"));
        assert_eq!(detail.neighborhood, None);
        assert_eq!(
            detail.description,
            "Rooms 10 minutes from the city centre.\n\nThe space\nRooms & suites with a private bathroom."
        );
    }

    #[test]
    fn apartment_fixture_fields_come_from_the_real_sections() {
        let detail = parse_detail_response(
            &fixture_json("p1b/pdp_apartment.json"),
            "38817969",
            "https://www.airbnb.com",
        )
        .unwrap();
        assert_eq!(detail.location, "Lyon, Auvergne-Rh\u{f4}ne-Alpes, France");
        assert_eq!(detail.property_type.as_deref(), Some("Entire rental unit"));
        assert_eq!(
            (
                detail.max_guests,
                detail.bedrooms,
                detail.beds,
                detail.bathrooms
            ),
            (Some(4), Some(1), Some(2), Some(1.0))
        );
        assert_eq!(detail.host_name.as_deref(), Some("Host A"));
        assert_eq!(detail.host_id.as_deref(), Some("1000001"));
        assert_eq!(detail.host_is_superhost, Some(true));
        assert_eq!(detail.host_response_rate.as_deref(), Some("100%"));
        assert_eq!(detail.host_response_time.as_deref(), Some("within an hour"));
        assert_eq!(detail.host_joined.as_deref(), Some("7 years hosting"));
        assert_eq!(detail.host_languages, vec!["French"]);
        assert_eq!(
            detail.amenities,
            vec!["Hair dryer", "Shampoo", "Hot water", "Smoke alarm"]
        );
        assert_eq!(detail.cancellation_policy.as_deref(), Some("Firm"));
        assert_eq!(
            detail.description,
            "Apartment on the slopes of the district, 5 minutes from the centre.\nMetro at 100 m & bakery nearby.\n\nThe space\n2nd floor, no elevator."
        );
        let shown = detail.to_string();
        assert!(shown.contains("| Response rate: 100% |"), "{shown}");
    }

    #[test]
    fn location_heading_alone_is_not_a_place() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "LOCATION_PDP",
             "section": {"title": "Where you\u{2019}ll be", "subtitle": null}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.location, "");
    }

    #[test]
    fn room_counts_keep_half_baths_and_are_not_overwritten() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "BOOK_IT_SIDEBAR", "section": {"maxGuestCapacity": 4}},
            {"sectionComponentType": "SBUI_SENTINEL", "sectionId": "OVERVIEW_DEFAULT_V2",
             "section": {"detailItems": [{"title": "Studio"}, {"title": "3 beds"}, {"title": "1.5 baths"}]}},
            {"sectionComponentType": "OVERVIEW_DEFAULT",
             "section": {"detailItems": [{"title": "16+ guests"}, {"title": "2 bedrooms"}]}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(
            (
                detail.max_guests,
                detail.bedrooms,
                detail.beds,
                detail.bathrooms
            ),
            (Some(4), Some(0), Some(3), Some(1.5))
        );
    }

    #[test]
    fn description_line_breaks_and_entities_survive() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "DESCRIPTION_DEFAULT",
             "section": {"htmlDescription": {"htmlText": "Near metro<br />Walk to Louvre &amp; Seine"}}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.description, "Near metro\nWalk to Louvre & Seine");
    }

    #[test]
    fn host_fields_come_from_meet_your_host_whatever_the_section_order() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "HOST_OVERVIEW_DEFAULT", "section": {"title": "Hosted by Alice"}},
            {"sectionComponentType": "MEET_YOUR_HOST",
             "section": {"cardData": {"name": "Alice", "userId": "555", "isSuperhost": true}}},
            {"sectionComponentType": "HOST_PROFILE_DEFAULT", "section": {}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.host_name.as_deref(), Some("Alice"));
        assert_eq!(detail.host_id.as_deref(), Some("555"));
        assert_eq!(detail.host_is_superhost, Some(true));
    }

    #[test]
    fn booking_sidebar_price_keeps_its_own_currency() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "BOOK_IT_SIDEBAR",
             "section": {"structuredDisplayPrice": {"primaryLine": {"price": "107\u{a0}\u{20ac}", "qualifier": "night"}}}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.known_price(), Some(107.0));
        assert_eq!(detail.currency, "\u{20ac}");
    }

    #[test]
    fn booking_sidebar_night_count_is_not_a_price() {
        let json = sections_json(&serde_json::json!([
            {"sectionComponentType": "BOOK_IT_SIDEBAR",
             "section": {"descriptionItems": [{"title": "2 nights minimum"}]}}
        ]));
        let detail = parse_detail_response(&json, "1", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.known_price(), None);
    }
}
