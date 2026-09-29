//! Input limits shared by `SearchParams::validate`, the MCP tool schemas
//! (`#[schemars(range(..))]` / `#[schemars(length(..))]`), the MCP runtime
//! checks and the CLI value parsers. With one table, the documented, the
//! advertised and the enforced ranges cannot drift apart (I8).

use crate::error::{AirbnbError, Result};

/// Smallest calendar window, in months.
pub const MONTHS_MIN: u32 = 1;
/// Largest calendar window, in months.
pub const MONTHS_MAX: u32 = 12;
/// Fewest review pages a sentiment analysis fetches.
pub const REVIEW_PAGES_MIN: u32 = 1;
/// Most review pages a sentiment analysis fetches.
pub const REVIEW_PAGES_MAX: u32 = 20;
/// Review pages fetched when `max_pages` is omitted.
pub const REVIEW_PAGES_DEFAULT: u32 = 5;
/// Fewest listing ids an id comparison accepts.
pub const COMPARE_IDS_MIN: usize = 2;
/// Most listing ids an id comparison accepts.
pub const COMPARE_IDS_MAX: usize = 10;
/// Fewest listings a location comparison may target.
pub const COMPARE_LISTINGS_MIN: u32 = 2;
/// Most listings a location comparison may target.
pub const COMPARE_LISTINGS_MAX: u32 = 100;
/// Listings a location comparison targets when `max_listings` is omitted.
pub const COMPARE_LISTINGS_DEFAULT: u32 = 20;
/// Fewest locations a market comparison accepts.
pub const MARKET_LOCATIONS_MIN: usize = 2;
/// Most locations a market comparison accepts.
pub const MARKET_LOCATIONS_MAX: usize = 5;
/// Longest accepted location, in characters after trimming. The value is
/// P3's `search_params::MAX_LOCATION_CHARS`, so the crate has one location limit.
pub const LOCATION_MAX_CHARS: usize = crate::domain::search_params::MAX_LOCATION_CHARS;
/// Longest accepted pagination cursor, in characters.
pub const CURSOR_MAX_CHARS: usize = 1024;
/// Longest accepted stay, in nights.
pub const MAX_STAY_NIGHTS: i64 = 365;
/// Furthest accepted check-in, in days after today.
pub const MAX_BOOKING_HORIZON_DAYS: i64 = 730;
/// Most adults per search.
pub const MAX_ADULTS: u32 = 16;
/// Most adults plus children per search.
pub const MAX_GUESTS: u32 = 16;
/// Most infants per search.
pub const MAX_INFANTS: u32 = 5;
/// Most pets per search.
pub const MAX_PETS: u32 = 5;

fn invalid(reason: String) -> AirbnbError {
    AirbnbError::InvalidParams { reason }
}

/// Return `value` when it lies in `min..=max`, else `InvalidParams` naming `field`.
pub fn in_range(field: &str, value: u32, min: u32, max: u32) -> Result<u32> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(invalid(format!(
            "{field} must be between {min} and {max}, got {value}"
        )))
    }
}

/// Check that a list argument has between `min` and `max` entries.
pub fn count_in_range(field: &str, count: usize, min: usize, max: usize) -> Result<()> {
    if (min..=max).contains(&count) {
        Ok(())
    } else {
        Err(invalid(format!(
            "{field} must contain between {min} and {max} entries, got {count}"
        )))
    }
}

/// Effective calendar window: `value`, or `default` when omitted, checked against 1–12.
pub fn months(value: Option<u32>, default: u32) -> Result<u32> {
    in_range("months", value.unwrap_or(default), MONTHS_MIN, MONTHS_MAX)
}

/// Effective review page count: `value`, or `default` when omitted, checked against 1–20.
pub fn review_pages(value: Option<u32>, default: u32) -> Result<u32> {
    in_range(
        "max_pages",
        value.unwrap_or(default),
        REVIEW_PAGES_MIN,
        REVIEW_PAGES_MAX,
    )
}

/// Effective location-comparison size: `value`, or 20 when omitted, checked against 2–100.
pub fn compare_listings(value: Option<u32>) -> Result<u32> {
    in_range(
        "max_listings",
        value.unwrap_or(COMPARE_LISTINGS_DEFAULT),
        COMPARE_LISTINGS_MIN,
        COMPARE_LISTINGS_MAX,
    )
}

/// A location, once trimmed, must be non-empty, at most `LOCATION_MAX_CHARS`
/// characters, free of control characters and contain at least one letter or
/// digit. These are P3's rules (SEC-1, VAL-4) with P3's messages: they moved
/// here from `SearchParams::validate` so that optional MCP `location`
/// arguments and the CLI value parsers apply them too. The location is never
/// echoed back.
pub fn check_location(location: &str) -> Result<()> {
    let location = location.trim();
    if location.is_empty() {
        return Err(invalid("location is required".into()));
    }
    let chars = location.chars().count();
    if chars > LOCATION_MAX_CHARS {
        return Err(invalid(format!(
            "location must be at most {LOCATION_MAX_CHARS} characters, got {chars}"
        )));
    }
    if location.chars().any(char::is_control) {
        return Err(invalid(
            "location must not contain control characters (newlines, tabs, ...)".into(),
        ));
    }
    if !location.chars().any(char::is_alphanumeric) {
        return Err(invalid(
            "location must contain at least one letter or digit".into(),
        ));
    }
    Ok(())
}

/// A pagination cursor must be at most `CURSOR_MAX_CHARS` characters long.
pub fn check_cursor(cursor: &str) -> Result<()> {
    let chars = cursor.chars().count();
    if chars > CURSOR_MAX_CHARS {
        return Err(invalid(format!(
            "cursor must be at most {CURSOR_MAX_CHARS} characters, got {chars}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_applies_default_then_checks_range() {
        assert_eq!(months(None, 3).unwrap(), 3);
        assert_eq!(months(Some(12), 3).unwrap(), 12);
        let err = months(Some(13), 3).unwrap_err().to_string();
        assert!(
            err.contains("months must be between 1 and 12, got 13"),
            "{err}"
        );
        assert!(months(Some(0), 12).is_err());
    }

    #[test]
    fn review_pages_range_is_1_to_20() {
        assert_eq!(review_pages(None, REVIEW_PAGES_DEFAULT).unwrap(), 5);
        assert_eq!(review_pages(Some(20), 5).unwrap(), 20);
        let err = review_pages(Some(21), 5).unwrap_err().to_string();
        assert!(
            err.contains("max_pages must be between 1 and 20, got 21"),
            "{err}"
        );
    }

    #[test]
    fn compare_listings_defaults_to_20_and_caps_at_100() {
        assert_eq!(compare_listings(None).unwrap(), 20);
        assert_eq!(compare_listings(Some(100)).unwrap(), 100);
        assert!(compare_listings(Some(1)).is_err());
        let err = compare_listings(Some(101)).unwrap_err().to_string();
        assert!(
            err.contains("max_listings must be between 2 and 100, got 101"),
            "{err}"
        );
    }

    #[test]
    fn count_in_range_names_the_field_and_the_count() {
        assert!(count_in_range("locations", 5, MARKET_LOCATIONS_MIN, MARKET_LOCATIONS_MAX).is_ok());
        let err = count_in_range("locations", 6, MARKET_LOCATIONS_MIN, MARKET_LOCATIONS_MAX)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("locations must contain between 2 and 5 entries, got 6"),
            "{err}"
        );
    }

    #[test]
    fn check_location_counts_characters_not_bytes() {
        assert!(check_location(&"é".repeat(LOCATION_MAX_CHARS)).is_ok());
        // Surrounding blanks do not count, as in P3's rule.
        assert!(check_location(&format!("  {}  ", "a".repeat(LOCATION_MAX_CHARS))).is_ok());
        let err = check_location(&"a".repeat(LOCATION_MAX_CHARS + 1))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("location must be at most 200 characters, got 201"),
            "{err}"
        );
        let err = check_location("   ").unwrap_err().to_string();
        assert!(err.contains("location is required"), "{err}");
    }

    #[test]
    fn check_location_applies_p3_character_rules() {
        let err = check_location("Paris\nINFO forged")
            .unwrap_err()
            .to_string();
        assert!(err.contains("control characters"), "{err}");
        let err = check_location("../..").unwrap_err().to_string();
        assert!(err.contains("at least one letter or digit"), "{err}");
        for ok in ["Paris, France", "São Paulo", "L'Île-Rousse", "東京"] {
            assert!(check_location(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn check_cursor_caps_length() {
        assert!(check_cursor(&"c".repeat(CURSOR_MAX_CHARS)).is_ok());
        let err = check_cursor(&"c".repeat(CURSOR_MAX_CHARS + 1))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cursor must be at most 1024 characters, got 1025"),
            "{err}"
        );
    }
}
