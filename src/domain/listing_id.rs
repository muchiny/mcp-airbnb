//! Listing ID validation.
//!
//! Airbnb listing IDs are numeric strings (the public `rooms/{id}` URL
//! always points at an integer). Accepting arbitrary strings at the port
//! boundary would allow a caller to smuggle slashes, query separators, or
//! other special characters into outbound URL paths and JSON GraphQL
//! variables. This module exposes a single `validate_listing_id` helper
//! that adapters call before using the id in any request.

use crate::error::{AirbnbError, Result, quote_input};

/// Maximum accepted length for a listing id: the number of decimal digits
/// in `u64::MAX`. Current Airbnb ids have up to 19 digits (for example
/// 1257736932578886647), so this is a hard ceiling, not a margin.
pub const MAX_LISTING_ID_LEN: usize = 20;

/// JSON-schema `pattern` for a listing id, as advertised in the MCP tool
/// schemas. `validate_listing_id` is stricter: it also rejects values above
/// `u64::MAX`.
pub const LISTING_ID_PATTERN: &str = r"^[1-9][0-9]{0,19}$";

/// Validate that `id` is a canonical Airbnb listing id: 1 to
/// [`MAX_LISTING_ID_LEN`] ASCII digits, no leading `0` (so neither `0` nor
/// `00123`), and a value that fits in `u64`. Returns
/// `AirbnbError::InvalidParams` on any violation, so callers get a clean
/// error instead of an opaque upstream 404, a malformed URL or a duplicate
/// cache entry.
pub fn validate_listing_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(AirbnbError::InvalidParams {
            reason: "listing id is required".into(),
        });
    }
    if id.len() > MAX_LISTING_ID_LEN {
        return Err(AirbnbError::InvalidParams {
            reason: format!(
                "listing id {} exceeds maximum length of {MAX_LISTING_ID_LEN} characters",
                quote_input(id)
            ),
        });
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AirbnbError::InvalidParams {
            reason: format!(
                "listing id {} must contain only ASCII digits",
                quote_input(id)
            ),
        });
    }
    if id.starts_with('0') {
        return Err(AirbnbError::InvalidParams {
            reason: format!("listing id {} must not start with 0", quote_input(id)),
        });
    }
    if id.parse::<u64>().is_err() {
        return Err(AirbnbError::InvalidParams {
            reason: format!(
                "listing id {} is larger than any Airbnb listing id",
                quote_input(id)
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_numeric_id() {
        assert!(validate_listing_id("12345").is_ok());
    }

    #[test]
    fn accepts_long_but_bounded_id() {
        let id = "1".repeat(MAX_LISTING_ID_LEN);
        assert!(validate_listing_id(&id).is_ok());
    }

    #[test]
    fn rejects_empty_id() {
        let err = validate_listing_id("").unwrap_err();
        assert!(matches!(err, AirbnbError::InvalidParams { .. }));
        assert!(err.to_string().contains("required"));
    }

    #[test]
    fn rejects_id_with_slash() {
        let err = validate_listing_id("123/admin").unwrap_err();
        assert!(err.to_string().contains("digits"));
    }

    #[test]
    fn rejects_id_with_query_separator() {
        let err = validate_listing_id("123?x=1").unwrap_err();
        assert!(err.to_string().contains("digits"));
    }

    #[test]
    fn rejects_id_with_fragment() {
        assert!(validate_listing_id("123#frag").is_err());
    }

    #[test]
    fn rejects_id_with_unicode() {
        assert!(validate_listing_id("123٤٥").is_err()); // Arabic-Indic digits
    }

    #[test]
    fn rejects_id_with_whitespace() {
        assert!(validate_listing_id("123 456").is_err());
        assert!(validate_listing_id("  12345").is_err());
    }

    #[test]
    fn rejects_overlong_id() {
        let id = "1".repeat(MAX_LISTING_ID_LEN + 1);
        let err = validate_listing_id(&id).unwrap_err();
        assert!(err.to_string().contains("maximum length"));
    }

    #[test]
    fn rejects_negative_sign() {
        assert!(validate_listing_id("-12345").is_err());
    }

    #[test]
    fn error_does_not_echo_unbounded_or_raw_input() {
        let hostile = format!("1\n2026-09-28T10:00:00Z  INFO forged{}", "x".repeat(5_000));
        let msg = validate_listing_id(&hostile).unwrap_err().to_string();
        assert!(!msg.contains('\n'), "raw newline echoed: {msg}");
        assert!(msg.len() < 200, "echo not bounded: {} bytes", msg.len());
    }

    #[test]
    fn non_digit_error_quotes_and_escapes_the_id() {
        let msg = validate_listing_id("12\n34").unwrap_err().to_string();
        assert!(msg.contains(r#""12\n34""#), "{msg}");
    }

    #[test]
    fn rejects_leading_zero_and_zero_ids() {
        for id in ["0", "0000000000", "00012345", "0042"] {
            let err = validate_listing_id(id).unwrap_err().to_string();
            assert!(err.contains("must not start with 0"), "{id}: {err}");
        }
    }

    #[test]
    fn accepts_current_19_digit_ids_and_small_ids() {
        assert!(validate_listing_id("1257736932578886647").is_ok());
        assert!(validate_listing_id("10").is_ok());
    }

    #[test]
    fn rejects_ids_above_u64_max() {
        let err = validate_listing_id("99999999999999999999")
            .unwrap_err()
            .to_string();
        assert!(err.contains("larger than any Airbnb listing id"), "{err}");
    }

    #[test]
    fn pattern_constant_describes_the_accepted_shape() {
        assert_eq!(LISTING_ID_PATTERN, r"^[1-9][0-9]{0,19}$");
    }
}
