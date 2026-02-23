use serde_json::Value;

use crate::domain::analytics::HostProfile;
use crate::error::{AirbnbError, Result};

/// Parse the GraphQL `StaysPdpSections` response into a `HostProfile`.
#[allow(clippy::too_many_lines)]
pub fn parse_host_response(json: &Value) -> Result<HostProfile> {
    // Try multiple response paths:
    // 1. Legacy GetUserProfile response
    // 2. PDP sections with MEET_YOUR_HOST or HOST* sections
    let profile = json
        .pointer("/data/presentation/userProfileContainer")
        .or_else(|| json.pointer("/data/user"));

    if let Some(profile) = profile {
        return parse_profile_object(profile);
    }

    // Find host section in PDP sections
    let sections = json
        .pointer("/data/presentation/stayProductDetailPage/sections/sections")
        .and_then(Value::as_array)
        .ok_or_else(|| AirbnbError::Parse {
            reason: "GraphQL host: could not find sections array".into(),
        })?;

    let host_section = sections
        .iter()
        .find(|s| {
            let stype = s
                .get("sectionComponentType")
                .and_then(Value::as_str)
                .unwrap_or_default();
            stype == "MEET_YOUR_HOST" || stype.contains("HOST")
        })
        .and_then(|s| s.get("section"))
        .ok_or_else(|| AirbnbError::Parse {
            reason: "GraphQL host: could not find host section".into(),
        })?;

    parse_meet_your_host_section(host_section)
}

/// Parse from a `MEET_YOUR_HOST` section (the real Airbnb GraphQL format).
#[allow(clippy::unnecessary_wraps, clippy::too_many_lines)]
fn parse_meet_your_host_section(section: &Value) -> Result<HostProfile> {
    let card = section.get("cardData");

    let name = card
        .and_then(|c| c.get("name"))
        .or_else(|| section.get("hostName"))
        .or_else(|| section.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_string();

    let host_id = card
        .and_then(|c| c.get("userId"))
        .or_else(|| section.get("hostId"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });

    let is_superhost = card
        .and_then(|c| c.get("isSuperhost"))
        .or_else(|| section.get("isSuperhost"))
        .and_then(Value::as_bool);

    // Response rate/time from hostDetails array: ["Response rate: 100%", "Responds within an hour"]
    let mut response_rate = None;
    let mut response_time = None;
    if let Some(details) = section.get("hostDetails").and_then(Value::as_array) {
        for detail_str in details.iter().filter_map(Value::as_str) {
            let lower = detail_str.to_lowercase();
            if lower.contains("response rate") {
                response_rate = Some(detail_str.to_string());
            } else if lower.contains("respond") {
                response_time = Some(detail_str.to_string());
            }
        }
    }
    if response_rate.is_none() {
        response_rate = section
            .get("hostResponseRate")
            .and_then(Value::as_str)
            .map(String::from);
    }
    if response_time.is_none() {
        response_time = section
            .get("hostRespondTimeCopy")
            .or_else(|| section.get("hostResponseTime"))
            .and_then(Value::as_str)
            .map(String::from);
    }

    let member_since = card
        .and_then(|c| c.pointer("/timeAsHost/years"))
        .and_then(Value::as_u64)
        .map(|y| format!("{y} years hosting"))
        .or_else(|| {
            section
                .get("hostMemberSince")
                .and_then(Value::as_str)
                .map(String::from)
        });

    // Languages from hostHighlights: [{"title": "Speaks English and French"}]
    let mut languages = Vec::new();
    if let Some(highlights) = section.get("hostHighlights").and_then(Value::as_array) {
        for highlight in highlights {
            if let Some(title) = highlight.get("title").and_then(Value::as_str) {
                let lower = title.to_lowercase();
                if lower.starts_with("speaks ") {
                    languages = title[7..]
                        .split([',', '&'])
                        .flat_map(|s| s.split(" and "))
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
    }
    if languages.is_empty()
        && let Some(langs) = section.get("hostLanguages").and_then(Value::as_array)
    {
        languages = langs
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
    }

    let total_listings = section
        .get("listingsCount")
        .or_else(|| section.get("hostListingCount"))
        .and_then(Value::as_u64)
        .map(|n| n as u32);

    let description = section
        .get("about")
        .or_else(|| section.get("description"))
        .and_then(Value::as_str)
        .map(String::from);

    let profile_picture_url = card
        .and_then(|c| c.get("profilePictureUrl"))
        .or_else(|| section.pointer("/profilePicture/baseUrl"))
        .or_else(|| section.get("profilePictureUrl"))
        .and_then(Value::as_str)
        .map(String::from);

    let identity_verified = card
        .and_then(|c| c.get("isIdentityVerified"))
        .or_else(|| section.get("isIdentityVerified"))
        .and_then(Value::as_bool);

    Ok(HostProfile {
        host_id,
        name,
        is_superhost,
        response_rate,
        response_time,
        member_since,
        languages,
        total_listings,
        description,
        profile_picture_url,
        identity_verified,
    })
}

/// Parse from a legacy user profile object (`GetUserProfile` format).
#[allow(clippy::unnecessary_wraps)]
fn parse_profile_object(profile: &Value) -> Result<HostProfile> {
    let name = profile
        .get("name")
        .or_else(|| profile.get("hostName"))
        .or_else(|| profile.get("firstName"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_string();

    let host_id = profile
        .get("id")
        .or_else(|| profile.get("hostId"))
        .or_else(|| profile.get("userId"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });

    let is_superhost = profile.get("isSuperhost").and_then(Value::as_bool);

    let response_rate = profile
        .get("responseRate")
        .or_else(|| profile.get("hostResponseRate"))
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            profile
                .get("responseRate")
                .and_then(Value::as_u64)
                .map(|n| format!("{n}%"))
        });

    let response_time = profile
        .get("responseTime")
        .or_else(|| profile.get("hostResponseTime"))
        .and_then(Value::as_str)
        .map(String::from);

    let member_since = profile
        .get("memberSince")
        .or_else(|| profile.get("createdAt"))
        .or_else(|| profile.get("hostMemberSince"))
        .and_then(Value::as_str)
        .map(String::from);

    let languages = profile
        .get("languages")
        .or_else(|| profile.get("hostLanguages"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let total_listings = profile
        .get("listingsCount")
        .or_else(|| profile.get("hostListingCount"))
        .and_then(Value::as_u64)
        .map(|n| n as u32);

    let description = profile
        .get("about")
        .or_else(|| profile.get("description"))
        .and_then(Value::as_str)
        .map(String::from);

    let profile_picture_url = profile
        .pointer("/profilePicture/baseUrl")
        .or_else(|| profile.get("profilePictureUrl"))
        .or_else(|| profile.get("pictureUrl"))
        .and_then(Value::as_str)
        .map(String::from);

    let identity_verified = profile
        .get("isIdentityVerified")
        .or_else(|| profile.get("identityVerified"))
        .and_then(Value::as_bool);

    Ok(HostProfile {
        host_id,
        name,
        is_superhost,
        response_rate,
        response_time,
        member_since,
        languages,
        total_listings,
        description,
        profile_picture_url,
        identity_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(profile.response_rate, Some("Response rate: 95%".to_string()));
        assert_eq!(profile.response_time, Some("Responds within a few hours".to_string()));
        assert_eq!(profile.description, Some("I love hosting!".to_string()));
        assert_eq!(profile.languages, vec!["English", "Spanish"]);
        assert_eq!(profile.profile_picture_url, Some("https://example.com/bob.jpg".to_string()));
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
}
