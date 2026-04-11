//! Listing ID validation.
//!
//! Airbnb listing IDs are numeric strings (the public `rooms/{id}` URL
//! always points at an integer). Accepting arbitrary strings at the port
//! boundary would allow a caller to smuggle slashes, query separators, or
//! other special characters into outbound URL paths and JSON GraphQL
//! variables. This module exposes a single `validate_listing_id` helper
//! that adapters call before using the id in any request.

use crate::error::{AirbnbError, Result};

/// Maximum accepted length for a listing id. Airbnb ids are currently
/// ~10-12 digits but we leave a generous margin for future growth while
/// still rejecting obvious junk.
const MAX_LISTING_ID_LEN: usize = 20;

/// Validate that `id` is a non-empty string of ASCII digits within
/// [`MAX_LISTING_ID_LEN`]. Returns `AirbnbError::InvalidParams` on any
/// violation so MCP clients receive a clean error instead of an opaque
/// upstream 404 or a malformed URL.
pub fn validate_listing_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(AirbnbError::InvalidParams {
            reason: "listing id is required".into(),
        });
    }
    if id.len() > MAX_LISTING_ID_LEN {
        return Err(AirbnbError::InvalidParams {
            reason: format!(
                "listing id '{id}' exceeds maximum length of {MAX_LISTING_ID_LEN} characters"
            ),
        });
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AirbnbError::InvalidParams {
            reason: format!("listing id '{id}' must contain only ASCII digits"),
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
}
