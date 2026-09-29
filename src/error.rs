use thiserror::Error;

#[derive(Error, Debug)]
pub enum AirbnbError {
    /// Transport-level failure (DNS, connect, TLS, timeout, body read, body
    /// too large). Built by `adapters::http::transport_error`; it carries no
    /// `reqwest` type, so the port contract does not depend on the HTTP crate.
    #[error("HTTP request failed: {reason}")]
    Http { reason: String },

    /// Airbnb answered with a non-success HTTP status. This is not a parse error.
    #[error("Airbnb returned HTTP {status} for {context}")]
    UpstreamStatus { status: u16, context: String },

    #[error("Failed to parse Airbnb response: {reason}")]
    Parse { reason: String },

    /// Airbnb's response no longer has the shape this client expects: GraphQL
    /// `errors` (e.g. a stale persisted-query hash), HTTP 422 on a persisted
    /// query, a missing response root, or an HTML page without listing data.
    #[error("Airbnb API changed for {operation}: {detail}")]
    UpstreamSchema { operation: String, detail: String },

    #[error("Listing not found: {id}")]
    ListingNotFound { id: String },

    /// The listing page was read and has no host profile section: listings
    /// offered by a business (`pdpType` `HOTEL`) show none.
    #[error("No host profile for listing {listing_id}: {reason}")]
    HostProfileUnavailable { listing_id: String, reason: String },

    #[error("Rate limit exceeded, try again later")]
    RateLimited,

    /// The GraphQL source failed with a fallback-worthy error and the HTML
    /// scraper failed too. Both causes are kept for the caller.
    #[error("GraphQL failed: {primary}; HTML fallback failed: {fallback}")]
    AllSourcesFailed {
        primary: Box<AirbnbError>,
        fallback: Box<AirbnbError>,
    },

    #[error("Invalid parameters: {reason}")]
    InvalidParams { reason: String },

    #[error("Insufficient data: {reason}")]
    InsufficientData { reason: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_saphyr::Error),
}

pub type Result<T> = std::result::Result<T, AirbnbError>;

/// Maximum number of characters of caller-supplied input echoed back in an
/// error message or a log field.
pub const MAX_ECHOED_INPUT_CHARS: usize = 32;

/// Quote caller-supplied input for an error message or a log field.
///
/// Control characters (newlines included) and quotes are escaped, and the text
/// is cut at [`MAX_ECHOED_INPUT_CHARS`] characters with a trailing `…`. A
/// hostile value can therefore neither forge log lines nor bloat responses.
pub fn quote_input(input: &str) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(MAX_ECHOED_INPUT_CHARS).collect();
    let truncated = chars.next().is_some();
    let mut quoted = String::with_capacity(head.len() + 4);
    quoted.push('"');
    quoted.extend(head.escape_debug());
    quoted.push('"');
    if truncated {
        quoted.push('…');
    }
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_display() {
        let err = AirbnbError::Parse {
            reason: "missing data".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing data"));
        assert!(msg.contains("parse"));
    }

    #[test]
    fn listing_not_found_display() {
        let err = AirbnbError::ListingNotFound { id: "42".into() };
        let msg = err.to_string();
        assert!(msg.contains("42"));
    }

    #[test]
    fn invalid_params_display() {
        let err = AirbnbError::InvalidParams {
            reason: "bad location".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("bad location"));
    }

    #[test]
    fn insufficient_data_display() {
        let err = AirbnbError::InsufficientData {
            reason: "no comparable listing".into(),
        };
        assert_eq!(err.to_string(), "Insufficient data: no comparable listing");
    }

    #[test]
    fn rate_limited_display() {
        let err = AirbnbError::RateLimited;
        let msg = err.to_string();
        assert!(msg.contains("Rate limit"));
    }

    #[test]
    fn error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{invalid").unwrap_err();
        let err: AirbnbError = json_err.into();
        assert!(matches!(err, AirbnbError::Json(_)));
        assert!(err.to_string().contains("JSON error"));
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: AirbnbError = io_err.into();
        assert!(matches!(err, AirbnbError::Io(_)));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn error_from_yaml() {
        let yaml_err = serde_saphyr::from_str::<u32>("{{").unwrap_err();
        let err: AirbnbError = yaml_err.into();
        assert!(matches!(err, AirbnbError::Yaml(_)));
        assert!(err.to_string().contains("YAML error"));
    }

    #[test]
    fn airbnb_error_is_send_sync_static() {
        // Both binaries turn AirbnbError into anyhow::Error, which requires this.
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<AirbnbError>();
    }

    #[test]
    fn airbnb_error_stays_below_clippy_large_err_threshold() {
        // clippy::result_large_err (default threshold 128 bytes) would flag every
        // `fn -> Result<T>` in the crate once AirbnbError reached that size.
        let size = std::mem::size_of::<AirbnbError>();
        assert!(
            size < 128,
            "AirbnbError is {size} bytes; box the largest variant (Task 4 Step 4b)"
        );
    }

    #[test]
    fn config_error_display() {
        let err = AirbnbError::Config("missing field".into());
        let msg = err.to_string();
        assert!(msg.contains("Configuration error"));
        assert!(msg.contains("missing field"));
    }

    #[test]
    fn upstream_schema_display_names_operation_and_detail() {
        let err = AirbnbError::UpstreamSchema {
            operation: "StaysSearch".into(),
            detail: "GraphQL errors: boom [ValidationError]".into(),
        };
        assert_eq!(
            err.to_string(),
            "Airbnb API changed for StaysSearch: GraphQL errors: boom [ValidationError]"
        );
    }

    #[test]
    fn host_profile_unavailable_names_the_listing_and_the_reason() {
        let err = AirbnbError::HostProfileUnavailable {
            listing_id: "42".into(),
            reason: "offered by a business".into(),
        };
        assert_eq!(
            err.to_string(),
            "No host profile for listing 42: offered by a business"
        );
    }

    #[test]
    fn upstream_status_is_not_reported_as_a_parse_error() {
        let err = AirbnbError::UpstreamStatus {
            status: 503,
            context: "GraphQL StaysSearch".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("HTTP 503"), "{msg}");
        assert!(msg.contains("GraphQL StaysSearch"), "{msg}");
        assert!(!msg.to_lowercase().contains("parse"), "{msg}");
    }

    #[test]
    fn http_error_carries_only_a_reason() {
        let err = AirbnbError::Http {
            reason: "timeout: operation timed out".into(),
        };
        assert_eq!(
            err.to_string(),
            "HTTP request failed: timeout: operation timed out"
        );
    }

    #[test]
    fn parse_error_no_longer_claims_html() {
        let err = AirbnbError::Parse {
            reason: "GraphQL StaysSearch JSON parse error".into(),
        };
        assert!(!err.to_string().contains("HTML"), "{err}");
    }

    #[test]
    fn quote_input_escapes_newlines_and_quotes() {
        assert_eq!(quote_input("a\nb\"c"), r#""a\nb\"c""#);
    }

    #[test]
    fn quote_input_truncates_long_input() {
        let quoted = quote_input(&"9".repeat(10_000));
        assert_eq!(
            quoted,
            format!("\"{}\"…", "9".repeat(MAX_ECHOED_INPUT_CHARS))
        );
    }

    #[test]
    fn quote_input_keeps_short_ids_intact() {
        assert_eq!(quote_input("12345"), "\"12345\"");
    }

    #[test]
    fn invalid_params_is_not_labelled_as_search() {
        let err = AirbnbError::InvalidParams {
            reason: "listing id is required".into(),
        };
        assert_eq!(
            err.to_string(),
            "Invalid parameters: listing id is required"
        );
    }
}
