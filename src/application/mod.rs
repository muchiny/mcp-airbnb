//! Application layer — orchestration and shared wiring.
//!
//! This module contains the "use cases" that wire adapters to ports and are
//! shared between the MCP server binary (`mcp-airbnb`) and the CLI binary
//! (`airbnb`). Per hexagonal architecture conventions, this layer is the only
//! one allowed to import from all other layers.

pub mod analytical_handlers;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::adapters::cache::memory_cache::MemoryCache;
use crate::adapters::composite::CompositeClient;
use crate::adapters::graphql::client::AirbnbGraphQLClient;
use crate::adapters::http::http_client_builder;
use crate::adapters::rate_limiter::RateLimiter;
use crate::adapters::scraper::client::AirbnbScraper;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::Config;
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

/// Environment variable naming an explicit config file (must exist).
pub const CONFIG_ENV_VAR: &str = "AIRBNB_CONFIG";

/// Where the configuration comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// `--config <path>` or `AIRBNB_CONFIG`: the file must exist.
    Explicit(PathBuf),
    /// The first existing file among [`config_candidates`].
    Discovered(PathBuf),
    /// No file anywhere: built-in defaults.
    Defaults,
}

/// `AIRBNB_CONFIG`, if set and non-empty.
pub fn config_path_from_env() -> Option<PathBuf> {
    std::env::var_os(CONFIG_ENV_VAR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Discovery candidates, in precedence order:
/// 1. `$XDG_CONFIG_HOME/mcp-airbnb/config.yaml`, else `$HOME/.config/mcp-airbnb/config.yaml`;
/// 2. `config.yaml` next to the running binary.
///
/// Only absolute paths are returned. The current working directory is
/// deliberately never searched: an MCP host starts the server in whatever
/// project is open, and that project's `config.yaml` must not configure it.
pub fn config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute())
                .map(|home| home.join(".config"))
        });
    if let Some(dir) = config_home {
        candidates.push(dir.join("mcp-airbnb").join("config.yaml"));
    }
    if let Some(dir) = binary_dir() {
        candidates.push(dir.join("config.yaml"));
    }
    candidates
}

fn binary_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .filter(|dir| dir.is_absolute())
}

/// Pure resolution step: an explicit path always wins (even if missing, so
/// the loader can report it), then the first existing candidate, then defaults.
pub fn resolve_config_source(explicit: Option<PathBuf>, candidates: &[PathBuf]) -> ConfigSource {
    if let Some(path) = explicit {
        return ConfigSource::Explicit(path);
    }
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .map_or(ConfigSource::Defaults, |found| {
            ConfigSource::Discovered(found.clone())
        })
}

/// Load (parse, normalise, validate) the configuration from `source`.
pub fn load_config_from(source: &ConfigSource) -> Result<Config> {
    match source {
        ConfigSource::Explicit(path) | ConfigSource::Discovered(path) => {
            tracing::info!(path = %path.display(), "loading configuration");
            Ok(crate::config::load_config(path)?)
        }
        ConfigSource::Defaults => {
            tracing::info!("no configuration file found; using built-in defaults");
            Ok(Config::default())
        }
    }
}

/// Resolve and load the configuration for either binary.
///
/// `explicit` is the CLI `--config` value (clap also fills it from
/// `AIRBNB_CONFIG`). The MCP server passes `None`, and `AIRBNB_CONFIG` is read here.
pub fn load_app_config(explicit: Option<PathBuf>) -> Result<Config> {
    let explicit = explicit.or_else(config_path_from_env);
    let source = resolve_config_source(explicit, &config_candidates());
    load_config_from(&source)
}

/// Build a fully-wired `AirbnbClient` from a loaded `Config`.
///
/// Chooses between a composite client (GraphQL + HTML fallback) and a plain
/// scraper based on `config.scraper.graphql_enabled`. The returned `Arc` is
/// shared by both binaries.
pub fn build_client(config: Config) -> Result<Arc<dyn AirbnbClient>> {
    let cache: Arc<dyn ListingCache> = Arc::new(MemoryCache::new(config.cache.max_entries));

    // One limiter for every request to Airbnb: the API-key homepage fetch,
    // GraphQL and the HTML scraper all draw from the same budget (I3).
    let rate_limiter = Arc::new(RateLimiter::new(config.scraper.rate_limit_per_second));

    let http_for_key = http_client_builder(&config.scraper)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client for API key manager: {e}"))?;

    let api_key_manager = Arc::new(ApiKeyManager::new(
        http_for_key,
        config.scraper.base_url.clone(),
        config.scraper.api_key_cache_secs,
        Arc::clone(&rate_limiter),
    ));

    if config.scraper.graphql_enabled {
        tracing::info!("GraphQL mode enabled — using composite client (GraphQL + HTML fallback)");
        let graphql = AirbnbGraphQLClient::new(
            &config.scraper,
            config.cache.clone(),
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
            Arc::clone(&rate_limiter),
        )
        .map_err(|e| anyhow::anyhow!("failed to create GraphQL client: {e}"))?;
        let scraper = AirbnbScraper::new(
            config.scraper,
            config.cache,
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
            rate_limiter,
        )
        .map_err(|e| anyhow::anyhow!("failed to create scraper client: {e}"))?;
        Ok(Arc::new(CompositeClient::new(
            Box::new(graphql),
            Box::new(scraper),
        )))
    } else {
        tracing::info!("GraphQL disabled — using HTML scraper only");
        Ok(Arc::new(
            AirbnbScraper::new(
                config.scraper,
                config.cache,
                cache,
                api_key_manager,
                rate_limiter,
            )
            .map_err(|e| anyhow::anyhow!("failed to create scraper client: {e}"))?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn build_client_paces_key_graphql_and_scraper_through_one_limiter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"<script>{"api_config":{"key":"k"}}</script>"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path_regex("^/api/v3/StaysPdpSections/"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("^/rooms/12345"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;

        let mut config = Config::default();
        config.scraper.base_url = server.uri();
        config.scraper.rate_limit_per_second = 5.0; // 200 ms between any two requests
        config.scraper.max_retries = 0;
        let client = build_client(config).expect("client builds");

        let start = Instant::now();
        let result = client.get_listing_detail("12345").await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "GraphQL and HTML both answer 500");
        // homepage at ~0 ms, GraphQL >= 200 ms, HTML page >= 400 ms.
        assert!(
            elapsed >= Duration::from_millis(390),
            "requests not paced by one shared limiter: {elapsed:?}"
        );
    }

    #[test]
    fn explicit_config_path_must_exist() {
        let typo = PathBuf::from("/nonexistent/airbnb-typo.yaml");
        let source = resolve_config_source(Some(typo.clone()), &[]);
        assert_eq!(source, ConfigSource::Explicit(typo));
        let err = load_config_from(&source).unwrap_err();
        assert!(err.to_string().contains("config file not found"), "{err}");
    }

    #[test]
    fn explicit_path_wins_over_discovered_files() {
        let dir = tempfile::tempdir().unwrap();
        let discovered = dir.path().join("config.yaml");
        std::fs::write(&discovered, "cache:\n  max_entries: 7\n").unwrap();
        let explicit = dir.path().join("explicit.yaml");
        std::fs::write(&explicit, "cache:\n  max_entries: 9\n").unwrap();

        let config =
            load_config_from(&resolve_config_source(Some(explicit), &[discovered])).unwrap();
        assert_eq!(config.cache.max_entries, 9);
    }

    #[test]
    fn discovery_takes_the_first_existing_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.yaml");
        let present = dir.path().join("present.yaml");
        std::fs::write(&present, "cache:\n  max_entries: 11\n").unwrap();

        let source = resolve_config_source(None, &[missing, present.clone()]);
        assert_eq!(source, ConfigSource::Discovered(present));
        assert_eq!(load_config_from(&source).unwrap().cache.max_entries, 11);
    }

    #[test]
    fn no_file_means_built_in_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let source = resolve_config_source(None, &[dir.path().join("absent.yaml")]);
        assert_eq!(source, ConfigSource::Defaults);
        let config = load_config_from(&source).unwrap();
        assert_eq!(
            config.cache.max_entries,
            Config::default().cache.max_entries
        );
    }

    #[test]
    fn discovery_never_looks_in_the_working_directory() {
        for candidate in config_candidates() {
            assert!(
                candidate.is_absolute(),
                "relative candidate {} would resolve against the working directory",
                candidate.display()
            );
        }
    }
}
