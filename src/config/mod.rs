pub mod types;

use std::path::Path;

use crate::error::{AirbnbError, Result};
use types::Config;

pub fn load_config(path: &Path) -> Result<Config> {
    if !path.is_file() {
        return Err(AirbnbError::Config(format!(
            "config file not found: {}",
            path.display()
        )));
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        AirbnbError::Config(format!(
            "failed to read config file {}: {e}",
            path.display()
        ))
    })?;
    let mut config: Config = serde_saphyr::from_str(&content)?;
    config.normalize();
    for key in config.deprecated_keys() {
        tracing::warn!(
            key,
            "config key is no longer supported and is ignored (see src/config/README.md)"
        );
    }
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn load_config_missing_file_is_an_error() {
        let err = load_config(Path::new("/nonexistent/mcp-airbnb/config.yaml")).unwrap_err();
        assert!(matches!(err, AirbnbError::Config(_)), "{err:?}");
        assert!(err.to_string().contains("config file not found"), "{err}");
    }

    #[test]
    fn load_config_valid_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "scraper:\n  max_retries: 5\n  request_timeout_secs: 60\ncache:\n  max_entries: 200"
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.scraper.max_retries, 5);
        assert_eq!(config.scraper.request_timeout_secs, 60);
        assert_eq!(config.cache.max_entries, 200);
    }

    #[test]
    fn load_config_partial_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  max_retries: 10").unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.scraper.max_retries, 10);
        // cache should get defaults
        assert_eq!(config.cache.search_ttl_secs, 900);
        assert_eq!(config.cache.detail_ttl_secs, 3600);
    }

    #[test]
    fn load_config_empty_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp).unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert!((config.scraper.rate_limit_per_second - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.scraper.max_retries, 2);
        assert_eq!(config.cache.max_entries, 500);
    }

    #[test]
    fn load_config_graphql_hash_override() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "scraper:\n  graphql_hashes:\n    stays_search: \"custom_hash_abc\""
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(
            config.scraper.graphql_hashes.stays_search,
            "custom_hash_abc"
        );
        assert!(!config.scraper.graphql_hashes.stays_pdp_sections.is_empty());
    }

    #[test]
    fn load_config_invalid_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "{{{{invalid yaml: [[[").unwrap();
        let result = load_config(tmp.path());
        assert!(matches!(result, Err(AirbnbError::Yaml(_))));
    }

    #[test]
    fn load_config_comment_only_yaml_returns_defaults() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "# every setting is commented out\n# scraper:\n#   max_retries: 9"
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.scraper.max_retries, 2);
        assert_eq!(config.cache.max_entries, 500);
    }

    #[test]
    fn load_config_integer_rate_limit_is_accepted_as_float() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  rate_limit_per_second: 1").unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert!((config.scraper.rate_limit_per_second - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn load_config_type_mismatch_is_yaml_error() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  max_retries: lots").unwrap();
        let err = load_config(tmp.path()).unwrap_err();
        assert!(
            matches!(err, AirbnbError::Yaml(_)),
            "unexpected error: {err:?}"
        );
        assert!(
            err.to_string().starts_with("YAML error"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn load_config_duplicate_key_is_rejected() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  max_retries: 1\n  max_retries: 9").unwrap();
        let err = load_config(tmp.path()).unwrap_err();
        assert!(
            matches!(err, AirbnbError::Yaml(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn load_config_parses_repository_config_yaml() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config.yaml"));
        let config = load_config(path).unwrap();
        assert_eq!(config.scraper.base_url, "https://www.airbnb.com");
        assert!(config.scraper.graphql_enabled);
        assert_eq!(config.scraper.graphql_hashes.stays_search.len(), 64);
    }

    #[test]
    fn load_config_rejects_values_that_disable_throttling() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  rate_limit_per_second: 0").unwrap();
        let err = load_config(tmp.path()).unwrap_err();
        assert!(matches!(err, AirbnbError::Config(_)), "{err:?}");
        assert!(
            err.to_string().contains("scraper.rate_limit_per_second"),
            "{err}"
        );
    }

    #[test]
    fn load_config_rejects_a_ttl_that_would_abort_the_server() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "cache:\n  search_ttl_secs: 18446744073709551615").unwrap();
        let err = load_config(tmp.path()).unwrap_err();
        assert!(matches!(err, AirbnbError::Config(_)), "{err:?}");
        assert!(err.to_string().contains("cache.search_ttl_secs"), "{err}");
    }

    #[test]
    fn load_config_rejects_plain_http_base_url() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  base_url: \"http://collector.example\"").unwrap();
        let err = load_config(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("https"), "{err}");
    }

    #[test]
    fn load_config_normalizes_a_trailing_slash() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "scraper:\n  base_url: \"https://www.airbnb.com/\"").unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.scraper.base_url, "https://www.airbnb.com");
    }

    #[test]
    fn load_config_rejects_unknown_keys() {
        for (yaml, key) in [
            ("scraper:\n  rate_limit_per_sec: 0.2", "rate_limit_per_sec"),
            ("cache:\n  backend: redis", "backend"),
            ("server:\n  port: 8080", "server"),
            (
                "scraper:\n  graphql_hashes:\n    stays_serch: \"abc\"",
                "stays_serch",
            ),
        ] {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            writeln!(tmp, "{yaml}").unwrap();
            let err = load_config(tmp.path()).unwrap_err();
            assert!(matches!(err, AirbnbError::Yaml(_)), "{yaml:?} -> {err:?}");
            assert!(err.to_string().contains(key), "{yaml:?} -> {err}");
        }
    }

    #[test]
    fn load_config_accepts_and_reports_removed_keys() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "scraper:\n  respect_robots_txt: true\n  graphql_hashes:\n    get_user_profile: \"abc\""
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(
            config.deprecated_keys(),
            vec![
                "scraper.respect_robots_txt",
                "scraper.graphql_hashes.get_user_profile"
            ]
        );
    }

    #[test]
    fn repository_config_yaml_has_no_removed_keys() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config.yaml"));
        let config = load_config(path).unwrap();
        assert!(
            config.deprecated_keys().is_empty(),
            "{:?}",
            config.deprecated_keys()
        );
    }
}
