//! Parser of the listing entity (`data.node`, a `DemandStayListing`) of a
//! `StaysPdpSections` response.
//!
//! Since 2026-11 Airbnb publishes the listing content (title, description,
//! amenities, host, location, ratings) in `data.node.pdpPresentation`. The
//! `sections` array is still there, but most of its sections are empty
//! placeholders. `parsers::detail` and `parsers::host` read this entity first
//! and fall back to the sections, so documents of both generations parse.

use serde_json::Value;

use super::pdp::{push_unique, str_field, u32_value};
use crate::adapters::text;
use crate::domain::analytics::HostProfile;
use crate::domain::listing::{CategoryRating, ReviewTag};

/// JSON pointer to the listing entity.
const NODE: &str = "/data/node";

/// Longest rule text copied next to a house-rule title.
const MAX_RULE_TEXT_CHARS: usize = 300;

/// Listing fields read from the listing entity. Everything is optional: the
/// detail parser fills what is missing from the sections and the metadata.
#[derive(Debug, Default)]
pub(crate) struct NodeDetail {
    pub(crate) name: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) neighborhood: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) rating: Option<f64>,
    pub(crate) review_count: Option<u32>,
    pub(crate) property_type: Option<String>,
    pub(crate) amenities: Vec<String>,
    pub(crate) house_rules: Vec<String>,
    pub(crate) photos: Vec<String>,
    pub(crate) latitude: Option<f64>,
    pub(crate) longitude: Option<f64>,
    pub(crate) rooms: text::RoomCounts,
    pub(crate) max_guests: Option<u32>,
    pub(crate) is_guest_favorite: Option<bool>,
    pub(crate) highlights: Vec<String>,
    pub(crate) category_ratings: Vec<CategoryRating>,
    pub(crate) review_tags: Vec<ReviewTag>,
}

/// The `pdpPresentation` object of the listing entity, when the document has one.
pub(crate) fn presentation(json: &Value) -> Option<&Value> {
    json.pointer(NODE)?
        .get("pdpPresentation")
        .filter(|presentation| presentation.is_object())
}

/// Text of a user-generated or localized field: a plain string, a `UGCText`
/// (`localizedString`, else the translation-preference copy, else `source`),
/// a `LocalizedText` (`text`), a `LocalizedContent` (`localizedContent`), or a
/// `PdpUGCText` wrapping one of them in `content`.
fn ugc_text(value: Option<&Value>) -> Option<String> {
    let outer = value?;
    let node = outer
        .get("content")
        .filter(|inner| inner.is_object())
        .unwrap_or(outer);
    let raw = node.as_str().or_else(|| {
        [
            "localizedString",
            "localizedStringWithTranslationPreference",
            "source",
            "text",
            "localizedContent",
        ]
        .iter()
        .find_map(|key| node.get(*key).and_then(Value::as_str))
    })?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn array<'a>(object: Option<&'a Value>, key: &str) -> impl Iterator<Item = &'a Value> {
    object
        .and_then(|o| o.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn positive(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n > 0.0)
}

/// Read the listing fields of the listing entity, or `None` when the document
/// has no `data.node.pdpPresentation` (documents captured before 2026-11).
pub(crate) fn node_detail(json: &Value) -> Option<NodeDetail> {
    let node = json.pointer(NODE)?;
    let pres = presentation(json)?;
    let overview = pres.get("overview");
    let overview_line =
        str_field(overview, "title").and_then(|title| text::split_type_and_place(&title));
    let place = pres.get("location");
    let quality = pres.get("quality");
    let stats = quality
        .and_then(|q| q.pointer("/listingRatingStats/overallRatingStats"))
        .or_else(|| node.pointer("/listingRatingStats/overallRatingStats"));

    let description = ugc_text(pres.pointer("/descriptions/longDescriptionHtml"))
        .or_else(|| ugc_text(pres.pointer("/descriptions/shortDescriptionHtml")))
        .map(|html| text::html_to_text(&html))
        .filter(|plain| !plain.is_empty());

    Some(NodeDetail {
        name: ugc_text(pres.get("title")).or_else(|| ugc_text(node.pointer("/description/name"))),
        location: str_field(place, "subtitle")
            .or_else(|| overview_line.as_ref().map(|(_, place)| place.clone()))
            .or_else(|| str_field(Some(pres), "localizedLocation")),
        neighborhood: str_field(place, "locationName").or_else(|| str_field(place, "subtitle")),
        description,
        rating: positive(stats.and_then(|s| s.get("ratingAverage"))),
        review_count: u32_value(stats.and_then(|s| s.get("ratingCount"))),
        property_type: str_field(pres.get("sharingConfig"), "propertyType")
            .or_else(|| overview_line.as_ref().map(|(kind, _)| kind.clone())),
        amenities: amenities(pres.get("amenities")),
        house_rules: house_rules(pres.get("rules")),
        photos: photos(pres),
        latitude: place
            .and_then(|p| p.get("latitude"))
            .or_else(|| node.pointer("/location/coordinate/latitude"))
            .and_then(Value::as_f64),
        longitude: place
            .and_then(|p| p.get("longitude"))
            .or_else(|| node.pointer("/location/coordinate/longitude"))
            .and_then(Value::as_f64),
        rooms: text::parse_room_counts(array(overview, "items").filter_map(Value::as_str)),
        max_guests: u32_value(
            pres.get("personCapacity")
                .or_else(|| node.get("personCapacity")),
        ),
        is_guest_favorite: quality
            .and_then(|q| q.get("isGuestFavorite"))
            .and_then(Value::as_bool),
        highlights: highlights(pres),
        category_ratings: category_ratings(quality),
        review_tags: review_tags(quality),
    })
}

/// Titles of the amenities the listing has (`pdpPresentation.amenities`).
fn amenities(block: Option<&Value>) -> Vec<String> {
    let mut titles = Vec::new();
    // `seeAllAmenitiesGroups` is the complete list; the preview is a subset.
    let groups = block
        .and_then(|a| a.get("seeAllAmenitiesGroups"))
        .filter(|groups| groups.as_array().is_some_and(|g| !g.is_empty()))
        .or_else(|| block.and_then(|a| a.get("previewAmenitiesGroups")))
        .and_then(Value::as_array);
    for item in groups
        .into_iter()
        .flatten()
        .flat_map(|group| array(Some(group), "amenities"))
    {
        // The "Not included" group lists amenities the listing lacks.
        if item.get("available").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        if let Some(title) = str_field(Some(item), "title") {
            push_unique(&mut titles, &title);
        }
    }
    titles
}

/// Photo URLs: the hero images first, then the photo tour.
fn photos(pres: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    for uri in array(pres.get("heroMedia"), "edges")
        .filter_map(|edge| edge.pointer("/node/image/uri"))
        .chain(
            array(pres.get("mediaTour"), "stops")
                .flat_map(|stop| array(Some(stop), "items"))
                .filter_map(|item| item.pointer("/image/uri")),
        )
        .filter_map(Value::as_str)
    {
        push_unique(&mut urls, uri);
    }
    urls
}

/// Listing highlights as `"headline: body"` lines.
fn highlights(pres: &Value) -> Vec<String> {
    array(Some(pres), "highlights")
        .filter_map(|highlight| {
            let headline = ugc_text(highlight.get("headline"))?;
            Some(match ugc_text(highlight.get("body")) {
                Some(body) => format!("{headline}: {body}"),
                None => headline,
            })
        })
        .collect()
}

fn category_ratings(quality: Option<&Value>) -> Vec<CategoryRating> {
    array(quality, "categoryRatings")
        .filter_map(|entry| {
            Some(CategoryRating {
                category: str_field(Some(entry), "label")
                    .or_else(|| str_field(Some(entry), "categoryType"))?,
                rating: entry
                    .get("localizedRating")
                    .and_then(Value::as_str)
                    .and_then(|rating| rating.trim().parse::<f64>().ok())
                    .filter(|rating| rating.is_finite())?,
            })
        })
        .collect()
}

fn review_tags(quality: Option<&Value>) -> Vec<ReviewTag> {
    array(quality, "reviewTags")
        .filter_map(|tag| {
            Some(ReviewTag {
                name: str_field(Some(tag), "localizedName")
                    .or_else(|| str_field(Some(tag), "name"))?,
                count: u32_value(tag.get("count"))?,
            })
        })
        .collect()
}

/// House rules as `"title"` or `"title: detail"` lines (`rules.groupItems`).
fn house_rules(rules: Option<&Value>) -> Vec<String> {
    let mut lines = Vec::new();
    for item in array(rules, "groupItems").flat_map(|group| array(Some(group), "items")) {
        let Some(title) = str_field(Some(item), "title") else {
            continue;
        };
        let detail = ugc_text(item.get("description")).map(|detail| {
            // Airbnb separates hours and AM/PM with narrow no-break spaces.
            let plain = text::html_to_text(&detail).replace(['\u{202f}', '\u{a0}'], " ");
            plain.chars().take(MAX_RULE_TEXT_CHARS).collect::<String>()
        });
        let line = match detail {
            Some(detail) if !detail.is_empty() => format!("{title}: {detail}"),
            _ => title,
        };
        push_unique(&mut lines, &line);
    }
    lines
}

/// Host profile of the listing entity (`pdpPresentation.hostInfo`), or `None`
/// when the document has no entity or the entity names no host.
pub(crate) fn node_host(json: &Value) -> Option<HostProfile> {
    let info = presentation(json)?.get("hostInfo")?;
    let passport = info.get("passportData");
    let name = str_field(passport, "name").or_else(|| {
        ugc_text(info.pointer("/overview/title"))
            .and_then(|title| text::host_name_from_hostinfo(&title))
    })?;

    let years = u32_value(passport.and_then(|p| p.pointer("/timeAsHost/years")));
    let months = u32_value(passport.and_then(|p| p.pointer("/timeAsHost/months")));
    let member_since = match (years, months) {
        (Some(years), _) if years > 0 => Some(format!("{years} years hosting")),
        (_, Some(months)) if months > 0 => Some(format!("{months} months hosting")),
        _ => None,
    };

    Some(HostProfile {
        host_id: passport
            .and_then(|p| p.get("userId"))
            .and_then(text::user_id_from_json),
        name,
        is_superhost: passport
            .and_then(|p| p.get("isSuperhost"))
            .and_then(Value::as_bool),
        response_rate: str_field(Some(info), "responseRateText")
            .map(|rate| text::normalize_response_rate(&rate)),
        response_time: str_field(Some(info), "responseTimeText")
            .map(|time| text::normalize_response_time(&time)),
        member_since,
        languages: array(Some(info), "highlights")
            .filter_map(|highlight| ugc_text(highlight.get("text")))
            .find_map(|line| text::languages_from_highlight(&line))
            .unwrap_or_default(),
        total_listings: None,
        description: ugc_text(info.get("about")),
        profile_picture_url: str_field(passport, "profilePictureUrl"),
        identity_verified: passport
            .and_then(|p| p.get("isVerified"))
            .and_then(Value::as_bool),
        rating: positive(passport.and_then(|p| p.get("ratingAverage"))),
        review_count: u32_value(passport.and_then(|p| p.get("ratingCount"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::fixture_2026_11;

    #[test]
    fn villa_entity_yields_the_full_listing() {
        let json = fixture_2026_11("StaysPdpSections.villa.response.json");
        let detail = node_detail(&json).expect("entity present");
        assert_eq!(detail.name.as_deref(), Some("Sample villa - 3 bedrooms"));
        assert_eq!(
            detail.location.as_deref(),
            Some("Porto-Vecchio, Corse, France")
        );
        assert_eq!(
            detail.neighborhood.as_deref(),
            Some("Porto-Vecchio, Corse, France")
        );
        assert_eq!(detail.property_type.as_deref(), Some("Entire villa"));
        assert_eq!(detail.rating, Some(4.94));
        assert_eq!(detail.review_count, Some(54));
        assert_eq!(detail.max_guests, Some(6));
        assert_eq!(detail.rooms.bedrooms, Some(3));
        assert_eq!(detail.rooms.beds, Some(4));
        assert_eq!(detail.rooms.bathrooms, Some(2.0));
        assert_eq!(detail.latitude, Some(41.5622));
        assert_eq!(detail.is_guest_favorite, Some(true));
        assert!(
            detail
                .description
                .as_deref()
                .is_some_and(|d| !d.contains('<'))
        );
        assert!(
            detail.amenities.iter().any(|a| a == "Wifi"),
            "{:?}",
            detail.amenities
        );
        assert!(
            !detail
                .amenities
                .iter()
                .any(|a| a.contains("security cameras")),
            "amenities of the \"Not included\" group are skipped"
        );
        assert!(detail.photos.len() >= 3, "{:?}", detail.photos);
        assert!(
            detail
                .house_rules
                .contains(&"Quiet hours: 11:00 PM - 8:00 AM".to_string())
        );
        assert_eq!(detail.category_ratings.len(), 6);
        assert_eq!(detail.category_ratings[0].category, "Cleanliness");
        assert!(
            detail
                .review_tags
                .iter()
                .any(|t| t.name == "Pool" && t.count == 24)
        );
        assert_eq!(detail.highlights.len(), 3);
    }

    #[test]
    fn villa_entity_yields_the_host() {
        let json = fixture_2026_11("StaysPdpSections.villa.response.json");
        let host = node_host(&json).expect("host present");
        assert_eq!(host.name, "Host A");
        assert_eq!(host.host_id.as_deref(), Some("1000001"));
        assert_eq!(host.is_superhost, Some(true));
        assert_eq!(host.identity_verified, Some(true));
        assert_eq!(host.response_rate.as_deref(), Some("100%"));
        assert_eq!(host.response_time.as_deref(), Some("within an hour"));
        assert_eq!(host.member_since.as_deref(), Some("8 years hosting"));
        assert_eq!(host.rating, Some(4.95));
        assert_eq!(host.review_count, Some(366));
        assert_eq!(host.languages, ["English", "French"]);
    }

    #[test]
    fn hotel_entity_falls_back_to_the_overview_line_and_entity_name() {
        let json = fixture_2026_11("StaysPdpSections.hotel.response.json");
        let detail = node_detail(&json).expect("entity present");
        assert_eq!(detail.name.as_deref(), Some("Sample hotel, classic room"));
        assert_eq!(detail.location.as_deref(), Some("V\u{e9}nissieux, France"));
        assert_eq!(detail.property_type.as_deref(), Some("Room in hotel"));
        assert_eq!(detail.rooms, text::RoomCounts::default());
        assert_eq!(detail.max_guests, Some(2));
        let host = node_host(&json).expect("business host present");
        assert_eq!(host.name, "Business Host B");
        assert_eq!(host.is_superhost, Some(false));
        assert_eq!(host.response_rate, None);
    }

    #[test]
    fn documents_without_the_entity_yield_none() {
        let old = crate::test_helpers::fixture_json("p1b/pdp_apartment.json");
        assert!(node_detail(&old).is_none());
        assert!(node_host(&old).is_none());
        assert!(
            presentation(&serde_json::json!({"data": {"node": {"pdpPresentation": null}}}))
                .is_none()
        );
    }

    #[test]
    fn ugc_text_reads_every_wrapper() {
        let cases = [
            (serde_json::json!("plain"), "plain"),
            (
                serde_json::json!({"localizedString": " a ", "source": "b"}),
                "a",
            ),
            (serde_json::json!({"source": "b"}), "b"),
            (serde_json::json!({"text": "c"}), "c"),
            (serde_json::json!({"localizedContent": "d"}), "d"),
            (
                serde_json::json!({"content": {"localizedString": "e"}}),
                "e",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(ugc_text(Some(&value)).as_deref(), Some(expected), "{value}");
        }
        assert_eq!(
            ugc_text(Some(&serde_json::json!({"localizedString": "  "}))),
            None
        );
        assert_eq!(ugc_text(Some(&serde_json::json!(null))), None);
        assert_eq!(ugc_text(None), None);
    }

    #[test]
    fn host_falls_back_to_the_overview_title_and_months() {
        let json = serde_json::json!({"data": {"node": {"pdpPresentation": {"hostInfo": {
            "passportData": {"name": " ", "timeAsHost": {"years": 0, "months": 5}},
            "overview": {"title": {"text": "Hosted by Sam"}}
        }}}}});
        let host = node_host(&json).expect("named by the overview");
        assert_eq!(host.name, "Sam");
        assert_eq!(host.member_since.as_deref(), Some("5 months hosting"));
        let nameless = serde_json::json!({"data": {"node": {"pdpPresentation": {"hostInfo": {
            "passportData": {"name": null}
        }}}}});
        assert!(node_host(&nameless).is_none());
    }
}
