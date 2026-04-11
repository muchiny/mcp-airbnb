use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use mcp_airbnb::application::{build_client, find_config_path};
use mcp_airbnb::config::load_config;
use mcp_airbnb::mcp::server::AirbnbMcpServer;

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

    // Load configuration and build the shared `AirbnbClient` via the
    // application layer (same wiring as the `airbnb` CLI binary).
    let config_path = find_config_path();
    let config = load_config(&config_path)?;
    let client = build_client(config)?;

    let server = AirbnbMcpServer::new(client);

    // Start MCP server over stdio
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
