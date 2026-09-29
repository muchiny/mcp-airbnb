//! `airbnb` CLI binary entry point: a thin shim over `mcp_airbnb::cli::run`.
#![warn(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    mcp_airbnb::cli::run().await
}
