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
