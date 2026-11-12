//! Helpers shared by the `StaysPdpSections` parsers (detail and host).

use serde_json::Value;

/// JSON pointer to the PDP sections array.
pub(crate) const SECTIONS: &str = "/data/presentation/stayProductDetailPage/sections/sections";

/// JSON pointer to the PDP metadata object (`pdpType`, `sharingConfig`, logging).
pub(crate) const METADATA: &str = "/data/presentation/stayProductDetailPage/sections/metadata";

const SBUI_SECTIONS: &str =
    "/data/presentation/stayProductDetailPage/sections/sbuiData/sectionConfiguration/root/sections";

/// `sectionData` of the SBUI block with the given `sectionId`.
///
/// Airbnb renders some sections (overview, host overview) through SBUI: the
/// `sections` array only holds an `SBUI_SENTINEL`, and the data lives here.
pub(crate) fn sbui_section_data<'a>(json: &'a Value, section_id: &str) -> Option<&'a Value> {
    json.pointer(SBUI_SECTIONS)?
        .as_array()?
        .iter()
        .find(|section| section.get("sectionId").and_then(Value::as_str) == Some(section_id))?
        .get("sectionData")
}

/// Non-empty, trimmed string field of an optional object.
pub(crate) fn str_field(object: Option<&Value>, key: &str) -> Option<String> {
    object?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A count published as a number or a numeric string.
pub(crate) fn u32_value(value: Option<&Value>) -> Option<u32> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .and_then(|n| u32::try_from(n).ok())
}

/// Most distinct items kept in one list (amenities, photos, house rules).
/// A real listing has a few dozen; the cap bounds the linear scan below on
/// a hostile document.
pub(crate) const MAX_UNIQUE_ITEMS: usize = 1000;

/// Append `value` unless the list already holds it or is full
/// ([`MAX_UNIQUE_ITEMS`]).
pub(crate) fn push_unique(list: &mut Vec<String>, value: &str) {
    if list.len() < MAX_UNIQUE_ITEMS && !list.iter().any(|existing| existing == value) {
        list.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_unique_skips_repeats_and_stops_at_the_cap() {
        let mut list = Vec::new();
        push_unique(&mut list, "Wifi");
        push_unique(&mut list, "Wifi");
        push_unique(&mut list, "Pool");
        assert_eq!(list, ["Wifi", "Pool"]);
        for n in 0..MAX_UNIQUE_ITEMS.saturating_mul(2) {
            push_unique(&mut list, &format!("item {n}"));
        }
        assert_eq!(list.len(), MAX_UNIQUE_ITEMS);
    }
}
