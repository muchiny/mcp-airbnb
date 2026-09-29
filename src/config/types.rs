use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub scraper: ScraperConfig,
    #[serde(default)]
    pub cache: CacheConfig,
}

/// Smallest accepted `scraper.rate_limit_per_second` (one request per 100 s).
pub const MIN_RATE_LIMIT_PER_SECOND: f64 = 0.01;
/// Largest accepted `scraper.rate_limit_per_second`.
pub const MAX_RATE_LIMIT_PER_SECOND: f64 = 10.0;
/// Largest accepted `scraper.request_timeout_secs`.
pub const MAX_REQUEST_TIMEOUT_SECS: u64 = 300;
/// Largest accepted `scraper.max_retries`.
pub const MAX_RETRIES: u32 = 10;
/// Smallest accepted `scraper.api_key_cache_secs`: shorter values re-fetch the
/// homepage before almost every GraphQL call.
pub const MIN_API_KEY_CACHE_SECS: u64 = 60;
/// Largest accepted TTL (30 days). Larger values overflow `Instant + ttl`.
pub const MAX_TTL_SECS: u64 = 30 * 24 * 60 * 60;
/// Largest accepted `cache.max_entries`. The LRU pre-allocates its table.
pub const MAX_CACHE_ENTRIES: usize = 100_000;

impl Config {
    /// Rewrite values that have one canonical form: trailing `/` is removed
    /// from `scraper.base_url`, so paths never start with `//`.
    pub fn normalize(&mut self) {
        self.scraper.base_url = self.scraper.base_url.trim_end_matches('/').to_string();
    }

    /// Removed config keys present in the loaded file. They are accepted (and
    /// ignored) so older files keep loading; `load_config` warns about each.
    pub fn deprecated_keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.scraper.removed_respect_robots_txt.is_some() {
            keys.push("scraper.respect_robots_txt");
        }
        if self
            .scraper
            .graphql_hashes
            .removed_get_user_profile
            .is_some()
        {
            keys.push("scraper.graphql_hashes.get_user_profile");
        }
        keys
    }

    /// Reject values that would panic the process, disable throttling, send
    /// traffic in plaintext, or make every request fail (I5). All problems are
    /// reported in one `AirbnbError::Config`.
    pub fn validate(&self) -> crate::error::Result<()> {
        let mut problems: Vec<String> = Vec::new();

        let scraper = &self.scraper;
        let rate = scraper.rate_limit_per_second;
        if !(MIN_RATE_LIMIT_PER_SECOND..=MAX_RATE_LIMIT_PER_SECOND).contains(&rate) {
            problems.push(format!(
                "scraper.rate_limit_per_second must be between {MIN_RATE_LIMIT_PER_SECOND} and {MAX_RATE_LIMIT_PER_SECOND}, got {rate}"
            ));
        }
        if !(1..=MAX_REQUEST_TIMEOUT_SECS).contains(&scraper.request_timeout_secs) {
            problems.push(format!(
                "scraper.request_timeout_secs must be between 1 and {MAX_REQUEST_TIMEOUT_SECS}, got {}",
                scraper.request_timeout_secs
            ));
        }
        if scraper.max_retries > MAX_RETRIES {
            problems.push(format!(
                "scraper.max_retries must be at most {MAX_RETRIES}, got {}",
                scraper.max_retries
            ));
        }
        if !(MIN_API_KEY_CACHE_SECS..=MAX_TTL_SECS).contains(&scraper.api_key_cache_secs) {
            problems.push(format!(
                "scraper.api_key_cache_secs must be between {MIN_API_KEY_CACHE_SECS} and {MAX_TTL_SECS}, got {}",
                scraper.api_key_cache_secs
            ));
        }
        if scraper.user_agent.trim().is_empty()
            || !scraper
                .user_agent
                .chars()
                .all(|c| c == ' ' || c.is_ascii_graphic())
        {
            problems.push("scraper.user_agent must be non-empty printable ASCII".to_string());
        }
        if let Some(problem) = base_url_problem(&scraper.base_url) {
            problems.push(problem);
        }
        // P1b (I4) sends both on every request, `locale` inside `Accept-Language`.
        if scraper.currency.len() != 3 || !scraper.currency.chars().all(|c| c.is_ascii_alphabetic())
        {
            problems.push(format!(
                "scraper.currency must be a 3-letter ISO 4217 code such as \"USD\", got {:?}",
                scraper.currency
            ));
        }
        let locale_ok = (1..=35).contains(&scraper.locale.len())
            && scraper
                .locale
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !locale_ok {
            problems.push(format!(
                "scraper.locale must be a language tag such as \"en\" or \"fr-FR\", got {:?}",
                scraper.locale
            ));
        }

        let cache = &self.cache;
        if !(1..=MAX_CACHE_ENTRIES).contains(&cache.max_entries) {
            problems.push(format!(
                "cache.max_entries must be between 1 and {MAX_CACHE_ENTRIES}, got {}",
                cache.max_entries
            ));
        }
        for (key, secs) in [
            ("cache.search_ttl_secs", cache.search_ttl_secs),
            ("cache.detail_ttl_secs", cache.detail_ttl_secs),
            ("cache.reviews_ttl_secs", cache.reviews_ttl_secs),
            ("cache.calendar_ttl_secs", cache.calendar_ttl_secs),
            ("cache.host_profile_ttl_secs", cache.host_profile_ttl_secs),
        ] {
            if secs > MAX_TTL_SECS {
                problems.push(format!(
                    "{key} must be at most {MAX_TTL_SECS} seconds (30 days), got {secs}"
                ));
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(crate::error::AirbnbError::Config(format!(
                "invalid configuration: {}",
                problems.join("; ")
            )))
        }
    }
}

/// `scraper.base_url` must be an `https://` origin without path, query,
/// fragment or credentials.
fn base_url_problem(base_url: &str) -> Option<String> {
    let Ok(url) = url::Url::parse(base_url) else {
        return Some(format!(
            "scraper.base_url {base_url:?} is not an absolute URL"
        ));
    };
    if url.scheme() != "https" {
        return Some(format!(
            "scraper.base_url must use https, got {:?}",
            url.scheme()
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Some("scraper.base_url must name a host".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Some("scraper.base_url must not contain credentials".to_string());
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Some(format!(
            "scraper.base_url must be an origin such as \"https://www.airbnb.com\", got {base_url:?}"
        ));
    }
    None
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScraperConfig {
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_second: f64,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    /// Removed: it never had any effect (robots.txt is not consulted). Still
    /// accepted, and ignored with a warning, so older files keep loading.
    #[doc(hidden)]
    #[serde(default, rename = "respect_robots_txt", skip_serializing)]
    pub removed_respect_robots_txt: Option<serde::de::IgnoredAny>,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_api_key_cache_secs")]
    pub api_key_cache_secs: u64,
    #[serde(default = "default_true")]
    pub graphql_enabled: bool,
    #[serde(default = "default_graphql_hashes")]
    pub graphql_hashes: GraphQLHashes,
    /// ISO 4217 currency pinned on every request (`currency=` query
    /// parameter): prices in responses are in this currency.
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Locale pinned on every request (`locale=` query parameter and
    /// `Accept-Language`). The parsers expect English labels.
    #[serde(default = "default_locale")]
    pub locale: String,
}

/// Persisted query hashes for Airbnb's internal GraphQL API.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphQLHashes {
    #[serde(default = "default_stays_search_hash")]
    pub stays_search: String,
    #[serde(default = "default_stays_pdp_sections_hash")]
    pub stays_pdp_sections: String,
    #[serde(default = "default_stays_pdp_reviews_hash")]
    pub stays_pdp_reviews: String,
    #[serde(default = "default_pdp_availability_calendar_hash")]
    pub pdp_availability_calendar: String,
    /// Removed: no operation used it. Still accepted, and ignored with a warning.
    #[doc(hidden)]
    #[serde(default, rename = "get_user_profile", skip_serializing)]
    pub removed_get_user_profile: Option<serde::de::IgnoredAny>,
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
            removed_respect_robots_txt: None,
            base_url: default_base_url(),
            api_key_cache_secs: default_api_key_cache_secs(),
            graphql_enabled: true,
            graphql_hashes: default_graphql_hashes(),
            currency: default_currency(),
            locale: default_locale(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default = "default_host_profile_ttl")]
    pub host_profile_ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            search_ttl_secs: default_search_ttl(),
            detail_ttl_secs: default_detail_ttl(),
            reviews_ttl_secs: default_reviews_ttl(),
            calendar_ttl_secs: default_calendar_ttl(),
            host_profile_ttl_secs: default_host_profile_ttl(),
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

fn default_currency() -> String {
    "USD".into()
}

fn default_locale() -> String {
    "en".into()
}

fn default_graphql_hashes() -> GraphQLHashes {
    GraphQLHashes {
        stays_search: default_stays_search_hash(),
        stays_pdp_sections: default_stays_pdp_sections_hash(),
        stays_pdp_reviews: default_stays_pdp_reviews_hash(),
        pdp_availability_calendar: default_pdp_availability_calendar_hash(),
        removed_get_user_profile: None,
    }
}

fn default_stays_search_hash() -> String {
    // airbnb.com web client, captured 2026-09-28
    "0afc7d440ee66286e44038530dc8d2af77d795e434e5cc5c8a8034c93cb377cf".into()
}

fn default_stays_pdp_sections_hash() -> String {
    "80c7889b4b0027d99ffea830f6c0d4911a6e863a957cbe1044823f0fc746bf1f".into()
}

fn default_stays_pdp_reviews_hash() -> String {
    // airbnb.com web client, captured 2026-09-28
    "cfdc3ffbe997a618795fc5a8f9a9b484054ce9be68c8788cd2ffda999934c5ae".into()
}

fn default_pdp_availability_calendar_hash() -> String {
    // airbnb.com web client, captured 2026-09-28
    "be60714ead0a30db42ce6471ddad6a8f3855df0ed400b79282dd0bb8cecdf201".into()
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

fn default_host_profile_ttl() -> u64 {
    3600
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
    }

    #[test]
    fn default_config_reports_no_deprecated_keys() {
        assert!(Config::default().deprecated_keys().is_empty());
    }

    #[test]
    fn cache_config_defaults() {
        let config = CacheConfig::default();
        assert_eq!(config.max_entries, 500);
        assert_eq!(config.search_ttl_secs, 900);
        assert_eq!(config.detail_ttl_secs, 3600);
        assert_eq!(config.reviews_ttl_secs, 3600);
        assert_eq!(config.calendar_ttl_secs, 1800);
        assert_eq!(config.host_profile_ttl_secs, 3600);
    }

    #[test]
    fn config_serde_roundtrip() {
        let original = Config::default();
        let yaml = serde_saphyr::to_string(&original).unwrap();
        let restored: Config = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(restored.scraper.max_retries, original.scraper.max_retries);
        assert_eq!(restored.cache.max_entries, original.cache.max_entries);
        assert!(
            (restored.scraper.rate_limit_per_second - original.scraper.rate_limit_per_second).abs()
                < f64::EPSILON
        );
        // The default user agent contains spaces, `;`, `(` and `,`; it must survive
        // the emit/parse round trip unchanged.
        assert_eq!(restored.scraper.user_agent, original.scraper.user_agent);
        assert_eq!(
            restored.scraper.graphql_hashes.stays_search,
            original.scraper.graphql_hashes.stays_search
        );
    }

    #[test]
    fn config_deserialize_with_overrides() {
        let yaml = "scraper:\n  max_retries: 5";
        let config: Config = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.scraper.max_retries, 5);
        // Other fields get defaults
        assert_eq!(config.scraper.request_timeout_secs, 30);
        assert_eq!(config.cache.search_ttl_secs, 900);
    }

    #[test]
    fn default_reviews_hash_is_the_2026_09_web_client_hash() {
        assert_eq!(
            GraphQLHashes::default().stays_pdp_reviews,
            "cfdc3ffbe997a618795fc5a8f9a9b484054ce9be68c8788cd2ffda999934c5ae"
        );
    }

    #[test]
    fn config_yaml_matches_default_graphql_hashes() {
        let yaml = include_str!("../../config.yaml");
        let hashes = GraphQLHashes::default();
        for (key, value) in [
            ("stays_search", &hashes.stays_search),
            ("stays_pdp_sections", &hashes.stays_pdp_sections),
            ("stays_pdp_reviews", &hashes.stays_pdp_reviews),
            (
                "pdp_availability_calendar",
                &hashes.pdp_availability_calendar,
            ),
        ] {
            assert!(
                yaml.contains(&format!("{key}: \"{value}\"")),
                "config.yaml {key} differs from the built-in default {value}"
            );
        }
    }

    #[test]
    fn default_search_hash_is_the_2026_09_web_client_hash() {
        assert_eq!(
            GraphQLHashes::default().stays_search,
            "0afc7d440ee66286e44038530dc8d2af77d795e434e5cc5c8a8034c93cb377cf"
        );
    }

    #[test]
    fn default_calendar_hash_is_the_2026_09_web_client_hash() {
        assert_eq!(
            GraphQLHashes::default().pdp_availability_calendar,
            "be60714ead0a30db42ce6471ddad6a8f3855df0ed400b79282dd0bb8cecdf201"
        );
    }

    #[test]
    fn scraper_config_pins_usd_and_en_by_default() {
        let config = ScraperConfig::default();
        assert_eq!(
            (config.currency.as_str(), config.locale.as_str()),
            ("USD", "en")
        );
        let parsed: ScraperConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(
            (parsed.currency.as_str(), parsed.locale.as_str()),
            ("USD", "en")
        );
        let custom: ScraperConfig =
            serde_json::from_str(r#"{"currency":"EUR","locale":"fr"}"#).unwrap();
        assert_eq!(
            (custom.currency.as_str(), custom.locale.as_str()),
            ("EUR", "fr")
        );
    }

    fn assert_rejected(config: &Config, key: &str) {
        let err = config.validate().expect_err("config must be rejected");
        assert!(matches!(err, crate::error::AirbnbError::Config(_)), "{err}");
        assert!(
            err.to_string().contains(key),
            "error should name {key}: {err}"
        );
    }

    #[test]
    fn default_config_is_valid() {
        Config::default()
            .validate()
            .expect("built-in defaults must validate");
    }

    #[test]
    fn rate_limits_that_disable_or_break_throttling_are_rejected() {
        for rate in [
            0.0,
            -0.5,
            0.001,
            10.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let mut config = Config::default();
            config.scraper.rate_limit_per_second = rate;
            assert_rejected(&config, "scraper.rate_limit_per_second");
        }
        for rate in [MIN_RATE_LIMIT_PER_SECOND, 0.5, MAX_RATE_LIMIT_PER_SECOND] {
            let mut config = Config::default();
            config.scraper.rate_limit_per_second = rate;
            config
                .validate()
                .unwrap_or_else(|e| panic!("rate {rate} should be valid: {e}"));
        }
    }

    #[test]
    fn zero_timeout_and_excessive_retries_are_rejected() {
        let mut config = Config::default();
        config.scraper.request_timeout_secs = 0;
        assert_rejected(&config, "scraper.request_timeout_secs");

        let mut config = Config::default();
        config.scraper.request_timeout_secs = MAX_REQUEST_TIMEOUT_SECS + 1;
        assert_rejected(&config, "scraper.request_timeout_secs");

        let mut config = Config::default();
        config.scraper.max_retries = MAX_RETRIES + 1;
        assert_rejected(&config, "scraper.max_retries");
    }

    #[test]
    fn ttls_that_would_overflow_instant_are_rejected() {
        let mut config = Config::default();
        config.cache.search_ttl_secs = u64::MAX;
        assert_rejected(&config, "cache.search_ttl_secs");

        let mut config = Config::default();
        config.cache.host_profile_ttl_secs = MAX_TTL_SECS + 1;
        assert_rejected(&config, "cache.host_profile_ttl_secs");

        let mut config = Config::default();
        config.cache.detail_ttl_secs = 0; // "do not cache" stays allowed
        config.validate().expect("zero TTL is allowed");
    }

    #[test]
    fn api_key_cache_below_a_minute_is_rejected() {
        let mut config = Config::default();
        config.scraper.api_key_cache_secs = 0;
        assert_rejected(&config, "scraper.api_key_cache_secs");
    }

    #[test]
    fn cache_capacity_bounds_are_enforced() {
        for entries in [0, MAX_CACHE_ENTRIES + 1, usize::MAX] {
            let mut config = Config::default();
            config.cache.max_entries = entries;
            assert_rejected(&config, "cache.max_entries");
        }
    }

    #[test]
    fn base_url_must_be_an_https_origin() {
        for url in [
            "http://www.airbnb.com",
            "ftp://www.airbnb.com",
            "https://www.airbnb.com/rooms",
            "https://www.airbnb.com/?x=1",
            "https://user:pw@www.airbnb.com",
            "not a url",
            "",
        ] {
            let mut config = Config::default();
            config.scraper.base_url = url.to_string();
            assert_rejected(&config, "scraper.base_url");
        }
        for url in [
            "https://www.airbnb.com",
            "https://www.airbnb.fr",
            "https://www.airbnb.com:443",
        ] {
            let mut config = Config::default();
            config.scraper.base_url = url.to_string();
            config
                .validate()
                .unwrap_or_else(|e| panic!("{url} should be valid: {e}"));
        }
    }

    #[test]
    fn user_agent_must_be_printable_ascii() {
        for user_agent in ["", "   ", "bad\nagent"] {
            let mut config = Config::default();
            config.scraper.user_agent = user_agent.to_string();
            assert_rejected(&config, "scraper.user_agent");
        }
    }

    #[test]
    fn currency_and_locale_must_be_header_safe_codes() {
        for currency in ["", "US", "USDX", "U$D", "US\n"] {
            let mut config = Config::default();
            config.scraper.currency = currency.to_string();
            assert_rejected(&config, "scraper.currency");
        }
        let too_long = "a".repeat(36);
        for locale in ["", "fr\nX-Evil: 1", "en US", too_long.as_str()] {
            let mut config = Config::default();
            config.scraper.locale = locale.to_string();
            assert_rejected(&config, "scraper.locale");
        }
        let mut config = Config::default();
        config.scraper.currency = "EUR".to_string();
        config.scraper.locale = "fr-FR".to_string();
        config.validate().expect("EUR and fr-FR are valid");
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let mut config = Config::default();
        config.scraper.rate_limit_per_second = 0.0;
        config.scraper.request_timeout_secs = 0;
        let message = config.validate().unwrap_err().to_string();
        assert!(
            message.contains("scraper.rate_limit_per_second"),
            "{message}"
        );
        assert!(
            message.contains("scraper.request_timeout_secs"),
            "{message}"
        );
    }

    #[test]
    fn normalize_strips_trailing_slashes_from_base_url() {
        let mut config = Config::default();
        config.scraper.base_url = "https://www.airbnb.com//".to_string();
        config.normalize();
        assert_eq!(config.scraper.base_url, "https://www.airbnb.com");
    }
}
