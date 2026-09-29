//! mcp-airbnb: MCP server and CLI over public Airbnb data.
//!
//! Production code must not `unwrap()` or `expect()`: release builds use
//! `panic = "abort"`, so any panic kills the whole server. Test code is exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;
pub mod error;
#[doc(hidden)]
pub mod fuzz_support;
pub mod mcp;
pub mod ports;

#[cfg(test)]
pub mod test_helpers;
