use scraper::{Html, Selector};

use crate::domain::analytics::HostProfile;
use crate::domain::listing::ListingDetail;
use crate::error::{AirbnbError, Result};

/// Parse host profile from a listing page HTML.
pub fn parse_host_profile(html: &str) -> Result<HostProfile> {
    // Try deferred state (niobeClientData) first
    if let Some(profile) = try_parse_host_from_deferred_state(html) {
        return Ok(profile);
    }
    Err(AirbnbError::Parse {
        reason: "could not extract host profile from listing page".into(),
    })
}

fn try_parse_host_from_deferred_state(html: &str) -> Option<HostProfile> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']").ok()?;

    for script in document.select(&selector) {
        let json_text = script.text().collect::<String>();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text)
            && let Some(entries) = data.get("niobeClientData").and_then(|v| v.as_array())
        {
            for entry in entries {
                if let Some(inner) = entry.as_array().and_then(|arr| arr.get(1))
                    && let Some(profile) = extract_host_from_pdp_sections(inner)
                {
                    return Some(profile);
                }
            }
        }
    }
    None
}

fn extract_host_from_pdp_sections(data: &serde_json::Value) -> Option<HostProfile> {
    let pdp = data
        .get("data")?
        .get("presentation")?
        .get("stayProductDetailPage")?;
    let sections_container = pdp.get("sections")?;
    let sections = sections_container.get("sections")?.as_array()?;

    let host_section = find_section(sections, "MEET_YOUR_HOST")?;
    let card_data = host_section.get("cardData")?;

    let name = card_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let host_id = card_data
        .get("userId")
        .or_else(|| card_data.get("id"))
        .or_else(|| card_data.get("hostId"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });

    let is_superhost = card_data
        .get("isSuperhost")
        .and_then(serde_json::Value::as_bool);

    // Response rate/time: try cardData first, fall back to section level
    let response_rate = card_data
        .get("responseRate")
        .or_else(|| host_section.get("hostResponseRate"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let response_time = card_data
        .get("responseTime")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            host_section
                .get("hostRespondTimeCopy")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

    // Member since: try cardData, then timeAsHost for years
    let member_since = card_data
        .get("memberSince")
        .or_else(|| card_data.get("createdAt"))
        .or_else(|| card_data.get("joinedDate"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            card_data
                .get("timeAsHost")
                .and_then(|t| t.get("years"))
                .and_then(serde_json::Value::as_u64)
                .map(|years| format!("{years} years hosting"))
        });

    // Languages: try cardData array, fall back to hostHighlights "Speaks ..." entries
    let languages = card_data
        .get("languages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|lang| lang.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| extract_languages_from_highlights(host_section));

    let total_listings = card_data
        .get("listingsCount")
        .or_else(|| card_data.get("hostListingCount"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    // Description: try cardData.about, then section-level about
    let description = card_data
        .get("about")
        .or_else(|| card_data.get("description"))
        .or_else(|| host_section.get("about"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Profile picture: try multiple field names including profilePictureUrl
    let profile_picture_url = card_data
        .get("profilePictureUrl")
        .or_else(|| card_data.get("profilePicture"))
        .or_else(|| card_data.get("avatarUrl"))
        .or_else(|| card_data.get("pictureUrl"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let identity_verified = card_data
        .get("isIdentityVerified")
        .or_else(|| card_data.get("identityVerified"))
        .or_else(|| card_data.get("isVerified"))
        .and_then(serde_json::Value::as_bool);

    Some(HostProfile {
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

/// Extract languages from hostHighlights (e.g. "Speaks English and French").
fn extract_languages_from_highlights(section: &serde_json::Value) -> Vec<String> {
    let Some(highlights) = section.get("hostHighlights").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    for highlight in highlights {
        if let Some(title) = highlight.get("title").and_then(|v| v.as_str()) {
            let lower = title.to_lowercase();
            if lower.starts_with("speaks ") || lower.starts_with("language") {
                // Parse "Speaks English and French" or "Speaks English, French, and Spanish"
                let after_speaks = if lower.starts_with("speaks ") {
                    &title[7..]
                } else {
                    title
                };
                return after_speaks
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

/// Parse listing detail page HTML into a `ListingDetail`.
pub fn parse_listing_detail(html: &str, listing_id: &str, base_url: &str) -> Result<ListingDetail> {
    // Try __NEXT_DATA__ JSON first
    if let Some(detail) = try_parse_next_data_detail(html, listing_id, base_url) {
        return Ok(detail);
    }

    // Try deferred state (current format with niobeClientData)
    if let Some(detail) = try_parse_deferred_state_detail(html, listing_id, base_url) {
        return Ok(detail);
    }

    // CSS fallback
    parse_detail_css(html, listing_id, base_url)
}

fn try_parse_next_data_detail(
    html: &str,
    listing_id: &str,
    base_url: &str,
) -> Option<ListingDetail> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r"script#__NEXT_DATA__").ok()?;
    let script = document.select(&selector).next()?;
    let json_text = script.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&json_text).ok()?;

    extract_detail_from_json(&data, listing_id, base_url)
}

fn try_parse_deferred_state_detail(
    html: &str,
    listing_id: &str,
    base_url: &str,
) -> Option<ListingDetail> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']").ok()?;

    for script in document.select(&selector) {
        let json_text = script.text().collect::<String>();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text) {
            // Try niobeClientData wrapper (current Airbnb format)
            if let Some(entries) = data.get("niobeClientData").and_then(|v| v.as_array()) {
                for entry in entries {
                    if let Some(inner) = entry.as_array().and_then(|arr| arr.get(1)) {
                        // Try PDP sections format
                        if let Some(detail) =
                            extract_detail_from_pdp_sections(inner, listing_id, base_url)
                        {
                            return Some(detail);
                        }
                        // Try legacy JSON format
                        if let Some(detail) = extract_detail_from_json(inner, listing_id, base_url)
                        {
                            return Some(detail);
                        }
                    }
                }
            }
            // Legacy: try direct JSON structure
            if let Some(detail) = extract_detail_from_json(&data, listing_id, base_url) {
                return Some(detail);
            }
        }
    }
    None
}

/// Extract listing detail from current Airbnb PDP sections format.
#[allow(clippy::too_many_lines)]
fn extract_detail_from_pdp_sections(
    data: &serde_json::Value,
    listing_id: &str,
    base_url: &str,
) -> Option<ListingDetail> {
    let pdp = data
        .get("data")?
        .get("presentation")?
        .get("stayProductDetailPage")?;
    let sections_container = pdp.get("sections")?;
    let sections = sections_container.get("sections")?.as_array()?;
    let metadata = sections_container.get("metadata")?;

    // Extract from metadata.sharingConfig
    let sharing = metadata.get("sharingConfig");
    let logging = metadata
        .get("loggingContext")
        .and_then(|lc| lc.get("eventDataLogging"));

    // Name: from sharingConfig.title or section AVAILABILITY_CALENDAR_DEFAULT.listingTitle
    let name = sharing
        .and_then(|s| s.get("title"))
        .and_then(|v| v.as_str())
        .or_else(|| find_section_field(sections, "AVAILABILITY_CALENDAR_DEFAULT", "listingTitle"))
        .unwrap_or("Unknown listing")
        .to_string();

    // Location
    let location = sharing
        .and_then(|s| s.get("location"))
        .and_then(|v| v.as_str())
        .or_else(|| find_section_field(sections, "LOCATION_PDP", "subtitle"))
        .unwrap_or("")
        .to_string();

    // Description: from DESCRIPTION_DEFAULT section
    let description = find_section(sections, "DESCRIPTION_DEFAULT")
        .and_then(|sec| {
            sec.get("htmlDescription")
                .and_then(|hd| hd.get("htmlText"))
                .and_then(|v| v.as_str())
        })
        .map(strip_html_tags)
        .unwrap_or_default();

    // Price: try BOOK_IT_SIDEBAR structuredDisplayPrice, then metadata
    let price_per_night = find_section(sections, "BOOK_IT_SIDEBAR")
        .and_then(|sec| {
            sec.pointer("/structuredDisplayPrice/primaryLine/discountedPrice")
                .or_else(|| sec.pointer("/structuredDisplayPrice/primaryLine/originalPrice"))
                .or_else(|| sec.pointer("/structuredDisplayPrice/primaryLine/price"))
                .or_else(|| sec.pointer("/structuredStayDisplayPrice/primaryLine/price"))
                .and_then(serde_json::Value::as_str)
                .and_then(extract_price_number)
        })
        .or_else(|| {
            logging
                .and_then(|l| l.get("listingPrice"))
                .and_then(serde_json::Value::as_f64)
        })
        .unwrap_or(0.0);

    // Currency
    let currency = "$".to_string();

    // Rating
    let rating = find_section(sections, "REVIEWS_DEFAULT")
        .and_then(|sec| sec.get("overallRating"))
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            sharing
                .and_then(|s| s.get("starRating"))
                .and_then(serde_json::Value::as_f64)
        })
        .or_else(|| {
            logging
                .and_then(|l| l.get("guestSatisfactionOverall"))
                .and_then(serde_json::Value::as_f64)
        });

    // Review count
    let review_count = find_section(sections, "REVIEWS_DEFAULT")
        .and_then(|sec| sec.get("overallCount"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            sharing
                .and_then(|s| s.get("reviewCount"))
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or(0) as u32;

    // Property type
    let property_type = sharing
        .and_then(|s| s.get("propertyType"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            logging
                .and_then(|l| l.get("roomType"))
                .and_then(|v| v.as_str())
        })
        .map(String::from);

    // Host name: from MEET_YOUR_HOST section
    let host_name = find_section(sections, "MEET_YOUR_HOST")
        .and_then(|sec| {
            sec.get("cardData")
                .and_then(|cd| cd.get("name"))
                .and_then(|v| v.as_str())
                .or_else(|| sec.get("titleText").and_then(|v| v.as_str()))
        })
        .map(String::from);

    // Amenities: from AMENITIES_DEFAULT section
    let amenities = find_section(sections, "AMENITIES_DEFAULT")
        .and_then(|sec| sec.get("previewAmenitiesGroups"))
        .and_then(|v| v.as_array())
        .map(|groups| {
            groups
                .iter()
                .flat_map(|group| {
                    group
                        .get("amenities")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|amenity| {
                            amenity
                                .get("title")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                })
                .collect()
        })
        .unwrap_or_default();

    // House rules: from POLICIES_DEFAULT section
    let house_rules = find_section(sections, "POLICIES_DEFAULT")
        .and_then(|sec| sec.get("houseRules"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|rule| rule.get("title").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Coordinates
    let latitude = find_section(sections, "LOCATION_PDP")
        .and_then(|sec| sec.get("lat"))
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            logging
                .and_then(|l| l.get("listingLat"))
                .and_then(serde_json::Value::as_f64)
        });

    let longitude = find_section(sections, "LOCATION_PDP")
        .and_then(|sec| sec.get("lng"))
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            logging
                .and_then(|l| l.get("listingLng"))
                .and_then(serde_json::Value::as_f64)
        });

    // Photos
    let photos = sharing
        .and_then(|s| s.get("imageUrl"))
        .and_then(|v| v.as_str())
        .map(|url| vec![url.to_string()])
        .unwrap_or_default();

    // Capacity info
    let max_guests = find_section(sections, "AVAILABILITY_CALENDAR_DEFAULT")
        .and_then(|sec| sec.get("maxGuestCapacity"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            sharing
                .and_then(|s| s.get("personCapacity"))
                .and_then(serde_json::Value::as_u64)
        })
        .or_else(|| {
            logging
                .and_then(|l| l.get("personCapacity"))
                .and_then(serde_json::Value::as_u64)
        })
        .map(|v| v as u32);

    // Bedrooms/beds/bathrooms from sharingConfig.title or descriptionItems
    let (bedrooms, beds, bathrooms) =
        extract_room_info_from_sharing_title(sharing).unwrap_or((None, None, None));

    // Check-in/out times from POLICIES_DEFAULT
    let (check_in_time, check_out_time) = extract_check_times(sections);

    // Host info from MEET_YOUR_HOST section cardData
    let host_section = find_section(sections, "MEET_YOUR_HOST");
    let card_data = host_section.and_then(|sec| sec.get("cardData"));

    let host_id = logging
        .and_then(|l| l.get("hostId"))
        .and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| "")))
        .map(String::from)
        .or_else(|| {
            card_data.and_then(|cd| cd.get("id")).and_then(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
        });

    let host_is_superhost = card_data
        .and_then(|cd| cd.get("isSuperhost"))
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            card_data
                .and_then(|cd| cd.get("badges"))
                .and_then(|b| b.as_array())
                .and_then(|arr| {
                    if arr
                        .iter()
                        .any(|badge| badge.as_str().is_some_and(|s| s.contains("uperhost")))
                    {
                        Some(true)
                    } else {
                        None
                    }
                })
        });

    let host_response_rate = card_data
        .and_then(|cd| cd.get("responseRate"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let host_response_time = card_data
        .and_then(|cd| cd.get("responseTime"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let host_joined = card_data
        .and_then(|cd| {
            cd.get("memberSince")
                .or_else(|| cd.get("createdAt"))
                .or_else(|| cd.get("joinedDate"))
        })
        .and_then(|v| v.as_str())
        .map(String::from);

    let host_total_listings = card_data
        .and_then(|cd| cd.get("listingsCount"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);

    let host_languages = card_data
        .and_then(|cd| cd.get("languages"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|lang| lang.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Cancellation policy from POLICIES_DEFAULT
    let cancellation_policy = find_section(sections, "POLICIES_DEFAULT")
        .and_then(|sec| {
            sec.get("cancellationPolicy")
                .and_then(|cp| cp.get("title").or_else(|| cp.get("policyName")))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    sec.get("cancellationPolicyForDisplay")
                        .and_then(|v| v.as_str())
                })
        })
        .map(String::from);

    // Neighborhood from LOCATION_PDP subtitle
    let neighborhood = find_section(sections, "LOCATION_PDP")
        .and_then(|sec| sec.get("subtitle").or_else(|| sec.get("neighborhoodName")))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Instant book from logging context
    let instant_book = logging
        .and_then(|l| l.get("instantBook").or_else(|| l.get("isInstantBook")))
        .and_then(serde_json::Value::as_bool);

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
        host_id,
        host_is_superhost,
        host_response_rate,
        host_response_time,
        host_joined,
        host_total_listings,
        host_languages,
        cancellation_policy,
        instant_book,
        cleaning_fee: None,
        service_fee: None,
        neighborhood,
    })
}

/// Find a section by its `sectionComponentType`.
fn find_section<'a>(
    sections: &'a [serde_json::Value],
    component_type: &str,
) -> Option<&'a serde_json::Value> {
    sections.iter().find_map(|s| {
        if s.get("sectionComponentType").and_then(|v| v.as_str()) == Some(component_type) {
            s.get("section")
        } else {
            None
        }
    })
}

/// Find a string field within a section by component type.
fn find_section_field<'a>(
    sections: &'a [serde_json::Value],
    component_type: &str,
    field: &str,
) -> Option<&'a str> {
    find_section(sections, component_type)
        .and_then(|sec| sec.get(field))
        .and_then(|v| v.as_str())
}

/// Extract a numeric price from a string like "$150", "€120.50".
fn extract_price_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cleaned.parse().ok()
}

/// Strip HTML tags from a string.
fn strip_html_tags(html: &str) -> String {
    html.replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split('<')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else if let Some(idx) = part.find('>') {
                part[(idx + 1)..].to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parse bedroom/bed/bathroom info from sharing title like "... · 1 bedroom · 1 bed · 1 shared bath"
fn extract_room_info_from_sharing_title(
    sharing: Option<&serde_json::Value>,
) -> Option<(Option<u32>, Option<u32>, Option<f64>)> {
    let title = sharing?.get("title")?.as_str()?;
    let parts: Vec<&str> = title.split('·').map(str::trim).collect();

    let mut bedrooms = None;
    let mut beds = None;
    let mut bathrooms = None;

    for part in &parts {
        let lower = part.to_lowercase();
        if lower.contains("bedroom") || lower.contains("studio") {
            bedrooms = extract_number_from_part(part);
            if lower.contains("studio") && bedrooms.is_none() {
                bedrooms = Some(0);
            }
        } else if lower.contains("bed") && !lower.contains("bedroom") {
            beds = extract_number_from_part(part);
        } else if lower.contains("bath") {
            bathrooms = extract_number_from_part(part).map(f64::from);
        }
    }

    Some((bedrooms, beds, bathrooms))
}

fn extract_number_from_part(part: &str) -> Option<u32> {
    part.split_whitespace()
        .find_map(|word| word.parse::<u32>().ok())
}

/// Extract check-in/check-out times from `POLICIES_DEFAULT` houseRules
fn extract_check_times(sections: &[serde_json::Value]) -> (Option<String>, Option<String>) {
    let Some(rules) = find_section(sections, "POLICIES_DEFAULT")
        .and_then(|sec| sec.get("houseRules"))
        .and_then(|v| v.as_array())
    else {
        return (None, None);
    };

    let mut check_in = None;
    let mut check_out = None;

    for rule in rules {
        let title = rule.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let lower = title.to_lowercase();
        if lower.starts_with("check-in") || lower.starts_with("checkin") {
            check_in = Some(title.to_string());
        } else if lower.starts_with("checkout") || lower.starts_with("check out") {
            check_out = Some(title.to_string());
        }
    }

    (check_in, check_out)
}

#[allow(clippy::too_many_lines)]
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
        .unwrap_or("$")
        .to_string();

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

fn parse_detail_css(html: &str, listing_id: &str, base_url: &str) -> Result<ListingDetail> {
    let document = Html::parse_document(html);

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
        currency: "$".into(),
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
        let html = "<html><body><h1>Beach Paradise</h1></body></html>";
        let detail = parse_listing_detail(html, "999", "https://www.airbnb.com").unwrap();
        assert_eq!(detail.name, "Beach Paradise");
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
        assert!(detail.name.contains("Paris"));
        assert_eq!(detail.location, "Paris");
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
    fn strip_html_tags_works() {
        assert_eq!(
            strip_html_tags("Hello <b>world</b><br />Next line"),
            "Hello world\nNext line"
        );
    }

    #[test]
    fn extract_room_info_from_title() {
        let sharing: serde_json::Value = serde_json::from_str(
            r#"{"title":"Rental unit · ⭐5.0 · 2 bedrooms · 3 beds · 1 bath"}"#,
        )
        .unwrap();
        let (bedrooms, beds, bathrooms) =
            extract_room_info_from_sharing_title(Some(&sharing)).unwrap();
        assert_eq!(bedrooms, Some(2));
        assert_eq!(beds, Some(3));
        assert_eq!(bathrooms, Some(1.0));
    }
}
