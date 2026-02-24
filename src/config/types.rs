use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub scraper: ScraperConfig,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScraperConfig {
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_second: f64,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    #[serde(default = "default_true")]
    pub respect_robots_txt: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_api_key_cache_secs")]
    pub api_key_cache_secs: u64,
    #[serde(default = "default_true")]
    pub graphql_enabled: bool,
    #[serde(default = "default_graphql_hashes")]
    pub graphql_hashes: GraphQLHashes,
}

/// Persisted query hashes for Airbnb's internal GraphQL API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GraphQLHashes {
    #[serde(default = "default_stays_search_hash")]
    pub stays_search: String,
    #[serde(default = "default_stays_pdp_sections_hash")]
    pub stays_pdp_sections: String,
    #[serde(default = "default_stays_pdp_reviews_hash")]
    pub stays_pdp_reviews: String,
    #[serde(default = "default_pdp_availability_calendar_hash")]
    pub pdp_availability_calendar: String,
    #[serde(default = "default_get_user_profile_hash")]
    pub get_user_profile: String,
}

impl Default for GraphQLHashes {
    fn default() -> Self {
        default_graphql_hashes()
    }
}

impl Default for ScraperConfig {
    fn default() -> Self {
        Self {
            user_agent: default_user_agent(),
            rate_limit_per_second: default_rate_limit(),
            request_timeout_secs: default_timeout(),
            max_retries: default_retries(),
            respect_robots_txt: true,
            base_url: default_base_url(),
            api_key_cache_secs: default_api_key_cache_secs(),
            graphql_enabled: true,
            graphql_hashes: default_graphql_hashes(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    #[serde(default = "default_search_ttl")]
    pub search_ttl_secs: u64,
    #[serde(default = "default_detail_ttl")]
    pub detail_ttl_secs: u64,
    #[serde(default = "default_reviews_ttl")]
    pub reviews_ttl_secs: u64,
    #[serde(default = "default_calendar_ttl")]
    pub calendar_ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            search_ttl_secs: default_search_ttl(),
            detail_ttl_secs: default_detail_ttl(),
            reviews_ttl_secs: default_reviews_ttl(),
            calendar_ttl_secs: default_calendar_ttl(),
        }
    }
}

fn default_user_agent() -> String {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into()
}

fn default_rate_limit() -> f64 {
    0.5
}

fn default_timeout() -> u64 {
    30
}

fn default_retries() -> u32 {
    2
}

fn default_true() -> bool {
    true
}

fn default_base_url() -> String {
    "https://www.airbnb.com".into()
}

fn default_api_key_cache_secs() -> u64 {
    86400 // 24 hours
}

fn default_graphql_hashes() -> GraphQLHashes {
    GraphQLHashes {
        stays_search: default_stays_search_hash(),
        stays_pdp_sections: default_stays_pdp_sections_hash(),
        stays_pdp_reviews: default_stays_pdp_reviews_hash(),
        pdp_availability_calendar: default_pdp_availability_calendar_hash(),
        get_user_profile: default_get_user_profile_hash(),
    }
}

fn default_stays_search_hash() -> String {
    "d4d9503616dc72ab220ed8dcf17f166816dccb2593e7b4625c91c3fce3a3b3d6".into()
}

fn default_stays_pdp_sections_hash() -> String {
    "80c7889b4b0027d99ffea830f6c0d4911a6e863a957cbe1044823f0fc746bf1f".into()
}

fn default_stays_pdp_reviews_hash() -> String {
    "dec1c8061483e78373602047450322fd474e79ba9afa8d3dbbc27f504030f91d".into()
}

fn default_pdp_availability_calendar_hash() -> String {
    "8f08e03c7bd16fcad3c92a3592c19a8b559a0d0855a84028d1163d4733ed9ade".into()
}

fn default_get_user_profile_hash() -> String {
    "a56d8909f271740ccfef23dd6c34d098f194f4a6e7157f244814c5610b8ad76a".into()
}

fn default_max_entries() -> usize {
    500
}

fn default_search_ttl() -> u64 {
    900
}

fn default_detail_ttl() -> u64 {
    3600
}

fn default_reviews_ttl() -> u64 {
    3600
}

fn default_calendar_ttl() -> u64 {
    1800
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = Config::default();
        assert!((config.scraper.rate_limit_per_second - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.scraper.request_timeout_secs, 30);
        assert_eq!(config.scraper.max_retries, 2);
        assert_eq!(config.scraper.base_url, "https://www.airbnb.com");
        assert!(config.scraper.respect_robots_txt);
    }

    #[test]
    fn cache_config_defaults() {
        let config = CacheConfig::default();
        assert_eq!(config.max_entries, 500);
        assert_eq!(config.search_ttl_secs, 900);
        assert_eq!(config.detail_ttl_secs, 3600);
        assert_eq!(config.reviews_ttl_secs, 3600);
        assert_eq!(config.calendar_ttl_secs, 1800);
    }

    #[test]
    fn config_serde_roundtrip() {
        let original = Config::default();
        let yaml = serde_yml::to_string(&original).unwrap();
        let restored: Config = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(restored.scraper.max_retries, original.scraper.max_retries);
        assert_eq!(restored.cache.max_entries, original.cache.max_entries);
        assert!(
            (restored.scraper.rate_limit_per_second - original.scraper.rate_limit_per_second).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn config_deserialize_with_overrides() {
        let yaml = "scraper:\n  max_retries: 5";
        let config: Config = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.scraper.max_retries, 5);
        // Other fields get defaults
        assert_eq!(config.scraper.request_timeout_secs, 30);
        assert_eq!(config.cache.search_ttl_secs, 900);
    }
}
