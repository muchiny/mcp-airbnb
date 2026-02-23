use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use mcp_airbnb::adapters::cache::memory_cache::MemoryCache;
use mcp_airbnb::adapters::composite::CompositeClient;
use mcp_airbnb::adapters::graphql::client::AirbnbGraphQLClient;
use mcp_airbnb::adapters::scraper::client::AirbnbScraper;
use mcp_airbnb::adapters::shared::ApiKeyManager;
use mcp_airbnb::config::load_config;
use mcp_airbnb::mcp::server::AirbnbMcpServer;

fn find_config_path() -> PathBuf {
    // Check common locations for config file
    let candidates = [
        PathBuf::from("config.yaml"),
        dirs_next().join("config.yaml"),
    ];

    for path in &candidates {
        if path.exists() {
            return path.clone();
        }
    }

    candidates[0].clone()
}

fn dirs_next() -> PathBuf {
    // Look in the directory where the binary is
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is reserved for MCP JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-airbnb server");

    // Load configuration
    let config_path = find_config_path();
    let config = load_config(&config_path)?;

    // Build dependencies
    let cache: Arc<dyn mcp_airbnb::ports::cache::ListingCache> =
        Arc::new(MemoryCache::new(config.cache.max_entries));

    // Shared API key manager (used by both scraper and GraphQL client)
    let http_for_key = reqwest::Client::builder()
        .user_agent(&config.scraper.user_agent)
        .timeout(std::time::Duration::from_secs(
            config.scraper.request_timeout_secs,
        ))
        .build()
        .expect("failed to build HTTP client for API key manager");
    let api_key_manager = Arc::new(ApiKeyManager::new(
        http_for_key,
        config.scraper.base_url.clone(),
        config.scraper.api_key_cache_secs,
    ));

    let client: Arc<dyn mcp_airbnb::ports::airbnb_client::AirbnbClient> = if config
        .scraper
        .graphql_enabled
    {
        tracing::info!("GraphQL mode enabled — using composite client (GraphQL + HTML fallback)");
        let graphql = AirbnbGraphQLClient::new(
            &config.scraper,
            config.cache.clone(),
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
        );
        let scraper = AirbnbScraper::new(
            config.scraper,
            config.cache,
            Arc::clone(&cache),
            Arc::clone(&api_key_manager),
        );
        Arc::new(CompositeClient::new(Box::new(graphql), Box::new(scraper)))
    } else {
        tracing::info!("GraphQL disabled — using HTML scraper only");
        Arc::new(AirbnbScraper::new(
            config.scraper,
            config.cache,
            cache,
            api_key_manager,
        ))
    };

    let server = AirbnbMcpServer::new(client);

    // Start MCP server over stdio
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
