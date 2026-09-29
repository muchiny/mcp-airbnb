//! Parser for Airbnb `StaySearchResult` items.
//!
//! The GraphQL `StaysSearch` response and the search page's embedded state
//! (`niobeClientData`) carry the same item shape, so both adapters convert
//! items through [`listing_from_stay_search_result`].

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::NaiveDate;
use serde_json::Value;

use crate::adapters::price;
use crate::adapters::text;
use crate::domain::listing::Listing;

/// Convert one `StaySearchResult` item into a [`Listing`].
///
/// Returns `None` when `demandStayListing.id` is missing or is not a base64
/// `DemandStayListing:<digits>` id. Values the item does not carry stay
/// explicitly unknown: `price_per_night` is `0.0` (see
/// [`Listing::known_price`]), `currency` is empty (the client labels it with
/// the pinned currency), `review_count` is `0`, `host_name` is `None` unless
/// the card says `Hosted by <name>`.
pub fn listing_from_stay_search_result(item: &Value, base_url: &str) -> Option<Listing> {
    let id = item
        .pointer("/demandStayListing/id")
        .and_then(Value::as_str)
        .and_then(decode_listing_id)?;
    let url = format!("{base_url}/rooms/{id}");
    let (name, place_line) = name_and_place_line(item);
    let (property_type, location) = match place_line.as_deref().and_then(text::split_type_and_place)
    {
        Some((kind, place)) => (
            Some(kind).filter(|kind| !kind.eq_ignore_ascii_case("place to stay")),
            place,
        ),
        None => (None, String::new()),
    };
    let display = item
        .get("structuredDisplayPrice")
        .map(|sdp| price::parse_structured_display_price(sdp, stay_nights(item)))
        .unwrap_or_default();
    let (rating, review_count) = rating_and_count(item);
    let photos: Vec<String> = item
        .get("contextualPictures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|picture| picture.get("picture").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let thumbnail_url = photos.first().cloned();
    let listing = item.get("demandStayListing");
    let coordinate = listing.and_then(|l| l.pointer("/location/coordinate"));

    Some(Listing {
        id,
        name,
        location,
        price_per_night: display.nightly.unwrap_or(0.0),
        currency: display.currency.unwrap_or_default(),
        rating,
        review_count,
        thumbnail_url,
        property_type,
        host_name: host_name(item),
        host_id: listing
            .and_then(|l| l.get("hostId").or_else(|| l.pointer("/primaryHost/id")))
            .and_then(text::user_id_from_json),
        url,
        is_superhost: superhost(item),
        is_guest_favorite: item
            .get("guestFavorite")
            .and_then(Value::as_bool)
            .or_else(|| has_badge(item, "GUEST_FAVORITE")),
        instant_book: listing
            .and_then(|l| l.get("instantBookEnabled"))
            .and_then(Value::as_bool),
        total_price: display.total,
        photos,
        latitude: coordinate
            .and_then(|c| c.get("latitude"))
            .and_then(Value::as_f64),
        longitude: coordinate
            .and_then(|c| c.get("longitude"))
            .and_then(Value::as_f64),
    })
}

/// `"DemandStayListing:<digits>"` (base64) → `"<digits>"`.
fn decode_listing_id(encoded: &str) -> Option<String> {
    let decoded = String::from_utf8(STANDARD.decode(encoded).ok()?).ok()?;
    let (_, id) = decoded.split_once(':')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
}

/// Listing name and the "Type in Place" line. Homes print the type line as
/// `title` and the name as `subtitle`; hotels print them the other way round.
fn name_and_place_line(item: &Value) -> (String, Option<String>) {
    let text_of = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let title = text_of(item.get("title"));
    let subtitle = text_of(item.get("subtitle"));
    let localized =
        text_of(item.pointer("/nameLocalized/localizedStringWithTranslationPreference"));
    let place_line = [title.as_ref(), subtitle.as_ref()]
        .into_iter()
        .flatten()
        .find(|line| {
            Some(*line) != localized.as_ref() && text::split_type_and_place(line).is_some()
        })
        .cloned();
    let name = localized
        .or_else(|| {
            [subtitle.as_ref(), title.as_ref()]
                .into_iter()
                .flatten()
                .find(|line| Some(*line) != place_line.as_ref())
                .cloned()
        })
        .unwrap_or_else(|| "Unknown listing".to_string());
    (name, place_line)
}

/// Nights of the dated search this item was priced for.
fn stay_nights(item: &Value) -> Option<u32> {
    let date = |key: &str| {
        item.get("listingParamOverrides")?
            .get(key)?
            .as_str()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    };
    let nights = (date("checkout")? - date("checkin")?).num_days();
    u32::try_from(nights).ok().filter(|n| *n > 0)
}

fn rating_and_count(item: &Value) -> (Option<f64>, u32) {
    let (rating, count) = item
        .get("avgRatingLocalized")
        .and_then(Value::as_str)
        .map_or((None, None), text::parse_rating_and_count);
    let (a11y_rating, a11y_count) = item
        .get("avgRatingA11yLabel")
        .and_then(Value::as_str)
        .map_or((None, None), text::parse_rating_a11y_label);
    (rating.or(a11y_rating), count.or(a11y_count).unwrap_or(0))
}

fn host_name(item: &Value) -> Option<String> {
    item.pointer("/structuredContent/primaryLine")
        .and_then(Value::as_array)?
        .iter()
        .filter(|line| line.get("type").and_then(Value::as_str) == Some("HOSTINFO"))
        .find_map(|line| {
            line.get("body")
                .and_then(Value::as_str)
                .and_then(text::host_name_from_hostinfo)
        })
}

fn has_badge(item: &Value, kind: &str) -> Option<bool> {
    item.get("badges")
        .and_then(Value::as_array)?
        .iter()
        .any(|badge| {
            badge
                .get("type")
                .or_else(|| badge.pointer("/loggingContext/badgeType"))
                .and_then(Value::as_str)
                .is_some_and(|badge_type| badge_type.contains(kind))
        })
        .then_some(true)
}

fn superhost(item: &Value) -> Option<bool> {
    has_badge(item, "SUPERHOST").or_else(|| {
        item.pointer("/structuredContent/primaryLine")
            .and_then(Value::as_array)?
            .iter()
            .any(|line| {
                line.get("body")
                    .and_then(Value::as_str)
                    .is_some_and(|body| body.contains("Superhost"))
            })
            .then_some(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE: &str = "https://www.airbnb.com";

    fn fixture_items() -> Vec<Value> {
        crate::test_helpers::fixture_json("p1b/stays_search.json")
            .pointer("/data/presentation/staysSearch/results/searchResults")
            .and_then(Value::as_array)
            .cloned()
            .expect("searchResults")
    }

    fn item_with(id: u64, extra: &Value) -> Value {
        let mut item = json!({
            "demandStayListing": {"id": STANDARD.encode(format!("DemandStayListing:{id}"))}
        });
        if let (Some(target), Some(source)) = (item.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        item
    }

    #[test]
    fn real_items_get_nightly_price_total_and_currency_from_the_same_block() {
        let listings: Vec<Listing> = fixture_items()
            .iter()
            .map(|item| listing_from_stay_search_result(item, BASE).expect("listing"))
            .collect();
        let summary: Vec<(&str, Option<f64>, Option<f64>, &str)> = listings
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
        assert_eq!(
            listings[0].url,
            "https://www.airbnb.com/rooms/1344016074576611955"
        );
    }

    #[test]
    fn hotel_items_swap_title_and_subtitle() {
        let items = fixture_items();
        let hotel = listing_from_stay_search_result(&items[2], BASE).unwrap();
        assert_eq!(hotel.name, "Example Hotel Lyon");
        assert_eq!(hotel.location, "3rd Arrondissement");
        assert_eq!(hotel.property_type.as_deref(), Some("Hotel"));
        let home = listing_from_stay_search_result(&items[0], BASE).unwrap();
        assert_eq!(home.name, "Example Suite - 2 bedrooms");
        assert_eq!(home.location, "1st Arrondissement");
        assert_eq!(home.property_type.as_deref(), Some("Apartment"));
        let generic = item_with(
            9,
            &json!({"title": "Place to stay in Paris", "subtitle": "Suite"}),
        );
        let generic = listing_from_stay_search_result(&generic, BASE).unwrap();
        assert_eq!(
            (generic.property_type, generic.location.as_str()),
            (None, "Paris")
        );
    }

    #[test]
    fn host_type_labels_are_not_host_names() {
        for item in fixture_items() {
            let listing = listing_from_stay_search_result(&item, BASE).unwrap();
            assert_eq!(listing.host_name, None, "{}", listing.id);
        }
        let hosted = item_with(
            1,
            &json!({"structuredContent": {"primaryLine": [{"type": "HOSTINFO", "body": "Hosted by Marie"}]}}),
        );
        assert_eq!(
            listing_from_stay_search_result(&hosted, BASE)
                .unwrap()
                .host_name
                .as_deref(),
            Some("Marie")
        );
    }

    #[test]
    fn ratings_and_review_counts_survive_thousands_separators() {
        let first = listing_from_stay_search_result(&fixture_items()[0], BASE).unwrap();
        assert_eq!((first.rating, first.review_count), (Some(4.95), 74));
        let big = item_with(2, &json!({"avgRatingLocalized": "4.95 (1,234)"}));
        assert_eq!(
            listing_from_stay_search_result(&big, BASE)
                .unwrap()
                .review_count,
            1234
        );
        let comma = item_with(3, &json!({"avgRatingLocalized": "4,93 (1 017)"}));
        let parsed = listing_from_stay_search_result(&comma, BASE).unwrap();
        assert_eq!((parsed.rating, parsed.review_count), (Some(4.93), 1017));
        let a11y_only = item_with(
            4,
            &json!({"avgRatingA11yLabel": "4.8 out of 5 average rating, 2,001 reviews"}),
        );
        let parsed = listing_from_stay_search_result(&a11y_only, BASE).unwrap();
        assert_eq!((parsed.rating, parsed.review_count), (Some(4.8), 2001));
    }

    #[test]
    fn guest_favorite_badge_is_read_from_the_logging_context() {
        let first = listing_from_stay_search_result(&fixture_items()[0], BASE).unwrap();
        assert_eq!(first.is_guest_favorite, Some(true));
        assert_eq!(first.latitude, Some(45.7677));
        assert_eq!(first.photos.len(), 1);
    }

    #[test]
    fn stay_total_without_breakdown_is_divided_by_the_booked_nights() {
        let item = item_with(
            5,
            &json!({
                "listingParamOverrides": {"checkin": "2026-10-01", "checkout": "2026-10-06"},
                "structuredDisplayPrice": {"primaryLine": {"price": "$750", "qualifier": "total"}}
            }),
        );
        let listing = listing_from_stay_search_result(&item, BASE).unwrap();
        assert_eq!(listing.known_price(), Some(150.0));
        assert_eq!(listing.total_price, Some(750.0));
    }

    #[test]
    fn unpriced_items_stay_explicitly_unknown() {
        let listing = listing_from_stay_search_result(&item_with(6, &json!({})), BASE).unwrap();
        assert_eq!(listing.known_price(), None);
        assert_eq!(listing.currency, "");
        assert_eq!(listing.review_count, 0);
        assert!(
            listing_from_stay_search_result(
                &json!({"demandStayListing": {"id": "not-base64"}}),
                BASE
            )
            .is_none()
        );
    }
}
