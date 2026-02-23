use thiserror::Error;

#[derive(Error, Debug)]
pub enum AirbnbError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse HTML response: {reason}")]
    Parse { reason: String },

    #[error("Listing not found: {id}")]
    ListingNotFound { id: String },

    #[error("Rate limit exceeded, try again later")]
    RateLimited,

    #[error("Invalid search parameters: {reason}")]
    InvalidParams { reason: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),
}

pub type Result<T> = std::result::Result<T, AirbnbError>;

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
}
