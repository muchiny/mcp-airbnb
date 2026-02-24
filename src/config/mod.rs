pub mod types;

use std::path::Path;

use crate::error::{AirbnbError, Result};
use types::Config;

pub fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        tracing::info!(
            "Config file not found at {}, using defaults",
            path.display()
        );
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        AirbnbError::Config(format!(
            "failed to read config file {}: {e}",
            path.display()
        ))
    })?;
    let config: Config = serde_yml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let result = load_config(Path::new("/tmp/nonexistent_mcp_config_12345.yaml"));
        assert!(result.is_ok());
        let config = result.unwrap();
        assert!((config.scraper.rate_limit_per_second - 0.5).abs() < f64::EPSILON);
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
        assert!(result.is_err());
    }
}
