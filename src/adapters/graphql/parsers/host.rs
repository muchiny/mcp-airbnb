use serde_json::Value;

use super::pdp::{self, str_field};
use crate::adapters::text;
use crate::domain::analytics::HostProfile;
use crate::error::{AirbnbError, Result};

/// Parse the GraphQL `StaysPdpSections` response into a `HostProfile`.
///
/// Returns [`AirbnbError::HostProfileUnavailable`] for listings offered by a
/// business (`pdpType` `HOTEL`), which have no host section, and
/// [`AirbnbError::UpstreamSchema`] when the document has no sections array,
/// when a regular listing has no host section, or when the host section
/// names no host.
pub fn parse_host_response(json: &Value) -> Result<HostProfile> {
    // Legacy `GetUserProfile` layouts.
    if let Some(profile) = json
        .pointer("/data/presentation/userProfileContainer")
        .or_else(|| json.pointer("/data/user"))
    {
        return Ok(parse_profile_object(profile));
    }

    let sections = json
        .pointer(pdp::SECTIONS)
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::UpstreamSchema {
            operation: "StaysPdpSections".into(),
            detail: "could not find sections array".into(),
        })?;
    let Some(section) = find_host_section(sections) else {
        return Err(missing_host_error(json));
    };
    host_profile_from_section(section, sbui_host_id(json)).ok_or_else(|| {
        AirbnbError::UpstreamSchema {
            operation: "StaysPdpSections".into(),
            detail: "the host section names no host".into(),
        }
    })
}

/// The section that describes the host, by priority: `MEET_YOUR_HOST`, then
/// `HOST_PROFILE_DEFAULT`, then any other non-sentinel type naming a host.
/// Section order in the payload does not matter.
pub(crate) fn find_host_section(sections: &[Value]) -> Option<&Value> {
    let component = |section: &Value| {
        section
            .get("sectionComponentType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    ["MEET_YOUR_HOST", "HOST_PROFILE_DEFAULT"]
        .iter()
        .find_map(|wanted| {
            sections
                .iter()
                .find(|&section| component(section) == *wanted)
        })
        .or_else(|| {
            sections.iter().find(|&section| {
                let kind = component(section);
                kind.contains("HOST") && kind != "SBUI_SENTINEL"
            })
        })
        .and_then(|section| section.get("section"))
}

/// Numeric host id published in the `HOST_OVERVIEW_DEFAULT` SBUI block.
pub(crate) fn sbui_host_id(json: &Value) -> Option<String> {
    pdp::sbui_section_data(json, "HOST_OVERVIEW_DEFAULT")?
        .pointer("/hostAvatar/loggingEventData/eventData/pdpContext/hostId")
        .and_then(text::user_id_from_json)
}

/// Why a PDP has no host section: a business listing (clear answer) or a
/// changed payload (schema error).
fn missing_host_error(json: &Value) -> AirbnbError {
    let metadata = json.pointer(pdp::METADATA);
    let pdp_type = str_field(metadata, "pdpType").unwrap_or_default();
    if pdp_type.eq_ignore_ascii_case("HOTEL") {
        let logging = metadata.and_then(|m| m.pointer("/loggingContext/eventDataLogging"));
        return AirbnbError::HostProfileUnavailable {
            listing_id: str_field(logging, "listingId").unwrap_or_else(|| "unknown".to_string()),
            reason: "the listing is offered by a business (pdpType HOTEL) and Airbnb shows no \
                     host section for it"
                .into(),
        };
    }
    AirbnbError::UpstreamSchema {
        operation: "StaysPdpSections".into(),
        detail: "no MEET_YOUR_HOST host section in the response".into(),
    }
}

/// Field of the host card, falling back to the section itself.
fn pick<'a>(card: Option<&'a Value>, section: &'a Value, key: &str) -> Option<&'a Value> {
    card.and_then(|c| c.get(key)).or_else(|| section.get(key))
}

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    let list: Vec<String> = value?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    (!list.is_empty()).then_some(list)
}

/// Build a host profile from a host section (`MEET_YOUR_HOST` layout; card
/// and section field variants). Returns `None` when the section names no host.
#[allow(clippy::too_many_lines)]
pub(crate) fn host_profile_from_section(
    section: &Value,
    fallback_host_id: Option<String>,
) -> Option<HostProfile> {
    let card = section.get("cardData");
    let name = pick(card, section, "name")
        .or_else(|| section.get("hostName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            section
                .get("title")
                .and_then(Value::as_str)
                .and_then(text::host_name_from_hostinfo)
        })?;

    let host_id = pick(card, section, "userId")
        .or_else(|| pick(card, section, "hostId"))
        .or_else(|| card.and_then(|c| c.get("id")))
        .and_then(text::user_id_from_json)
        .or(fallback_host_id);

    // hostDetails: ["Response rate: 100%", "Responds within an hour"]
    let mut response_rate = None;
    let mut response_time = None;
    for detail in section
        .get("hostDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let lower = detail.to_lowercase();
        if lower.contains("response rate") {
            response_rate = Some(text::normalize_response_rate(detail));
        } else if lower.contains("respond") {
            response_time = Some(text::normalize_response_time(detail));
        }
    }
    let response_rate = response_rate.or_else(|| {
        pick(card, section, "responseRate")
            .or_else(|| pick(card, section, "hostResponseRate"))
            .and_then(Value::as_str)
            .map(text::normalize_response_rate)
    });
    let response_time = response_time.or_else(|| {
        pick(card, section, "responseTime")
            .or_else(|| pick(card, section, "hostRespondTimeCopy"))
            .or_else(|| pick(card, section, "hostResponseTime"))
            .and_then(Value::as_str)
            .map(text::normalize_response_time)
    });

    let member_since = card
        .and_then(|c| c.pointer("/timeAsHost/years"))
        .and_then(Value::as_u64)
        .map(|years| format!("{years} years hosting"))
        .or_else(|| {
            pick(card, section, "memberSince")
                .or_else(|| pick(card, section, "hostMemberSince"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    let languages = section
        .get("hostHighlights")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|highlight| highlight.get("title").and_then(Value::as_str))
        .find_map(text::languages_from_highlight)
        .or_else(|| string_list(pick(card, section, "languages")))
        .or_else(|| string_list(pick(card, section, "hostLanguages")))
        .unwrap_or_default();

    Some(HostProfile {
        host_id,
        name,
        is_superhost: pick(card, section, "isSuperhost").and_then(Value::as_bool),
        response_rate,
        response_time,
        member_since,
        languages,
        total_listings: pick(card, section, "listingsCount")
            .or_else(|| pick(card, section, "hostListingCount"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        description: pick(card, section, "about")
            .or_else(|| pick(card, section, "description"))
            .and_then(Value::as_str)
            .map(str::to_string),
        profile_picture_url: pick(card, section, "profilePictureUrl")
            .or_else(|| section.pointer("/profilePicture/baseUrl"))
            .and_then(Value::as_str)
            .map(str::to_string),
        identity_verified: pick(card, section, "isIdentityVerified")
            .or_else(|| pick(card, section, "isVerified"))
            .and_then(Value::as_bool),
    })
}

/// First string field of `object` among `keys`.
fn first_str(object: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

/// Parse a legacy user profile object (`GetUserProfile` format).
fn parse_profile_object(profile: &Value) -> HostProfile {
    HostProfile {
        host_id: ["id", "hostId", "userId"]
            .iter()
            .find_map(|key| profile.get(*key))
            .and_then(text::user_id_from_json),
        name: first_str(profile, &["name", "hostName", "firstName"])
            .unwrap_or_else(|| "Unknown".to_string()),
        is_superhost: profile.get("isSuperhost").and_then(Value::as_bool),
        response_rate: first_str(profile, &["responseRate", "hostResponseRate"])
            .map(|rate| text::normalize_response_rate(&rate))
            .or_else(|| {
                profile
                    .get("responseRate")
                    .and_then(Value::as_u64)
                    .map(|n| format!("{n}%"))
            }),
        response_time: first_str(profile, &["responseTime", "hostResponseTime"])
            .map(|time| text::normalize_response_time(&time)),
        member_since: first_str(profile, &["memberSince", "createdAt", "hostMemberSince"]),
        languages: string_list(
            profile
                .get("languages")
                .or_else(|| profile.get("hostLanguages")),
        )
        .unwrap_or_default(),
        total_listings: profile
            .get("listingsCount")
            .or_else(|| profile.get("hostListingCount"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        description: first_str(profile, &["about", "description"]),
        profile_picture_url: profile
            .pointer("/profilePicture/baseUrl")
            .or_else(|| profile.get("profilePictureUrl"))
            .or_else(|| profile.get("pictureUrl"))
            .and_then(Value::as_str)
            .map(str::to_string),
        identity_verified: profile
            .get("isIdentityVerified")
            .or_else(|| profile.get("identityVerified"))
            .and_then(Value::as_bool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::fixture_json;

    #[test]
    fn parse_host_basic() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "userProfileContainer": {
                        "name": "Alice",
                        "id": "12345",
                        "isSuperhost": true,
                        "responseRate": "98%",
                        "responseTime": "within an hour",
                        "memberSince": "2015",
                        "languages": ["English", "French"],
                        "listingsCount": 5,
                        "about": "Experienced host",
                        "isIdentityVerified": true,
                        "profilePicture": {
                            "baseUrl": "https://example.com/photo.jpg"
                        }
                    }
                }
            }
        });

        let profile = parse_host_response(&json).unwrap();
        assert_eq!(profile.name, "Alice");
        assert_eq!(profile.host_id, Some("12345".to_string()));
        assert_eq!(profile.is_superhost, Some(true));
        assert_eq!(profile.response_rate, Some("98%".to_string()));
        assert_eq!(profile.languages, vec!["English", "French"]);
        assert_eq!(profile.total_listings, Some(5));
        assert_eq!(profile.identity_verified, Some(true));
    }

    #[test]
    fn parse_host_from_section() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "sections": {
                            "sections": [{
                                "sectionComponentType": "MEET_YOUR_HOST",
                                "sectionId": "MEET_YOUR_HOST",
                                "section": {
                                    "cardData": {
                                        "name": "Bob",
                                        "userId": "67890",
                                        "isSuperhost": false,
                                        "profilePictureUrl": "https://example.com/bob.jpg"
                                    },
                                    "about": "I love hosting!",
                                    "hostDetails": ["Response rate: 95%", "Responds within a few hours"],
                                    "hostHighlights": [
                                        { "title": "Speaks English and Spanish" },
                                        { "title": "Lives in Paris, France" }
                                    ]
                                }
                            }]
                        }
                    }
                }
            }
        });

        let profile = parse_host_response(&json).unwrap();
        assert_eq!(profile.name, "Bob");
        assert_eq!(profile.host_id, Some("67890".to_string()));
        assert_eq!(profile.is_superhost, Some(false));
        assert_eq!(profile.response_rate, Some("95%".to_string()));
        assert_eq!(
            profile.response_time,
            Some("within a few hours".to_string())
        );
        assert_eq!(profile.description, Some("I love hosting!".to_string()));
        assert_eq!(profile.languages, vec!["English", "Spanish"]);
        assert_eq!(
            profile.profile_picture_url,
            Some("https://example.com/bob.jpg".to_string())
        );
    }

    #[test]
    fn parse_host_missing_data_returns_error() {
        let json = serde_json::json!({
            "data": {
                "presentation": {}
            }
        });
        let result = parse_host_response(&json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("could not find"));
    }

    #[test]
    fn apartment_fixture_host_has_clean_labels_and_a_numeric_id() {
        let profile = parse_host_response(&fixture_json("p1b/pdp_apartment.json")).unwrap();
        assert_eq!(profile.name, "Host A");
        assert_eq!(profile.host_id.as_deref(), Some("1000001"));
        assert_eq!(profile.is_superhost, Some(true));
        assert_eq!(profile.response_rate.as_deref(), Some("100%"));
        assert_eq!(profile.response_time.as_deref(), Some("within an hour"));
        assert_eq!(profile.member_since.as_deref(), Some("7 years hosting"));
        assert_eq!(profile.languages, vec!["French"]);
        assert_eq!(profile.description.as_deref(), Some("Host bio redacted."));
        assert_eq!(profile.identity_verified, Some(true));
        let shown = profile.to_string();
        assert!(shown.contains("Response rate: 100%\n"), "{shown}");
        assert!(!shown.contains("Response rate: Response rate"), "{shown}");
        assert!(shown.contains("ID: 1000001\n"), "{shown}");
    }

    #[test]
    fn hotel_fixture_reports_business_listing_instead_of_a_parse_failure() {
        match parse_host_response(&fixture_json("p1b/pdp_hotel.json")) {
            Err(AirbnbError::HostProfileUnavailable { listing_id, reason }) => {
                assert_eq!(listing_id, "1257736932578886647");
                assert!(reason.contains("HOTEL"), "{reason}");
            }
            other => panic!("expected HostProfileUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn meet_your_host_wins_over_an_earlier_host_overview_section() {
        let json = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"sections": {"sections": [
            {"sectionComponentType": "HOST_OVERVIEW_DEFAULT", "section": {"title": "Hosted by Alice"}},
            {"sectionComponentType": "MEET_YOUR_HOST",
             "section": {"cardData": {"name": "Alice", "userId": "555", "isSuperhost": true}}}
        ]}}}}});
        let profile = parse_host_response(&json).unwrap();
        assert_eq!(profile.name, "Alice");
        assert_eq!(profile.host_id.as_deref(), Some("555"));
        assert_eq!(profile.is_superhost, Some(true));
    }

    #[test]
    fn regular_listing_without_a_host_section_is_an_upstream_schema_error() {
        let mut json = fixture_json("p1b/pdp_apartment.json");
        json.pointer_mut("/data/presentation/stayProductDetailPage/sections/sections")
            .and_then(Value::as_array_mut)
            .unwrap()
            .retain(|section| section["sectionComponentType"] != "MEET_YOUR_HOST");
        let err = parse_host_response(&json).unwrap_err();
        assert!(matches!(err, AirbnbError::UpstreamSchema { .. }), "{err}");
        assert!(err.to_string().contains("host"), "{err}");
    }
}
