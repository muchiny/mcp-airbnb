//! `airbnb` CLI binary entry point — thin shim over `mcp_airbnb::cli::run`.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    mcp_airbnb::cli::run().await
}
