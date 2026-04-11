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
use crate::adapters::scraper::client::AirbnbScraper;
use crate::adapters::shared::ApiKeyManager;
use crate::config::types::Config;
use crate::ports::airbnb_client::AirbnbClient;
use crate::ports::cache::ListingCache;

/// Find a config file path using the following precedence:
/// 1. `AIRBNB_CONFIG` environment variable
/// 2. `./config.yaml` in the current working directory
/// 3. `config.yaml` next to the binary
///
/// Always returns a path, even if no file exists (the loader falls back
/// to defaults when the file is missing).
pub fn find_config_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("AIRBNB_CONFIG") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return p;
        }
    }

    let candidates = [
        PathBuf::from("config.yaml"),
        binary_dir().join("config.yaml"),
    ];
    for path in &candidates {
        if path.exists() {
            return path.clone();
        }
    }
    candidates[0].clone()
}

fn binary_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Build a fully-wired `AirbnbClient` from a loaded `Config`.
///
/// Chooses between a composite client (GraphQL + HTML fallback) and a plain
/// scraper based on `config.scraper.graphql_enabled`. The returned `Arc` is
/// shared by both binaries.
pub fn build_client(config: Config) -> Result<Arc<dyn AirbnbClient>> {
    let cache: Arc<dyn ListingCache> = Arc::new(MemoryCache::new(config.cache.max_entries));

    // Shared HTTP client for the API key manager.
    let http_for_key = reqwest::Client::builder()
        .user_agent(&config.scraper.user_agent)
        .timeout(std::time::Duration::from_secs(
            config.scraper.request_timeout_secs,
        ))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client for API key manager: {e}"))?;

    let api_key_manager = Arc::new(ApiKeyManager::new(
        http_for_key,
        config.scraper.base_url.clone(),
        config.scraper.api_key_cache_secs,
    ));

    if config.scraper.graphql_enabled {
        tracing::info!("GraphQL mode enabled — using composite client (GraphQL + HTML fallback)");
        let graphql = AirbnbGraphQLClient::new(
            &config.scraper,
            config.cache.clone(),
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
        )
        .map_err(|e| anyhow::anyhow!("failed to create GraphQL client: {e}"))?;
        let scraper = AirbnbScraper::new(
            config.scraper,
            config.cache,
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
        )
        .map_err(|e| anyhow::anyhow!("failed to create scraper client: {e}"))?;
        Ok(Arc::new(CompositeClient::new(
            Box::new(graphql),
            Box::new(scraper),
        )))
    } else {
        tracing::info!("GraphQL disabled — using HTML scraper only");
        Ok(Arc::new(
            AirbnbScraper::new(config.scraper, config.cache, cache, api_key_manager)
                .map_err(|e| anyhow::anyhow!("failed to create scraper client: {e}"))?,
        ))
    }
}
