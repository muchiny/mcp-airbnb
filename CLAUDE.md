# mcp-airbnb Development Guide

## Project Overview
MCP (Model Context Protocol) server in Rust that enables AI assistants to search and browse Airbnb listings via web scraping of public pages.

## Architecture
Hexagonal architecture: `domain/` (pure types) → `ports/` (traits) → `adapters/` (implementations) → `mcp/` (protocol layer).

- **Domain**: Pure types with serde/schemars derives, no I/O
- **Ports**: `AirbnbClient` and `ListingCache` traits
- **Adapters**: `scraper/` (reqwest + HTML parsing), `cache/` (LRU in-memory)
- **MCP**: rmcp 0.16 server with `#[tool_router]`/`#[tool]`/`#[tool_handler]` macros

## Build & Test
```bash
cargo build                    # Build
cargo test                     # Run all tests
cargo clippy                   # Lint
cargo fmt                      # Format
cargo run                      # Start MCP server (stdio)
RUST_LOG=debug cargo run       # With debug logging (to stderr)
```

## Key Conventions
- Rust edition 2024, minimum version 1.93
- `thiserror` for domain errors, `anyhow` for application errors
- All logging to stderr (stdout reserved for MCP JSON-RPC)
- Rate limiting: 1 request per 2 seconds by default
- In-memory LRU cache with configurable TTLs
- Configuration via `config.yaml` (serde_yaml)

## MCP Tools
| Tool | Description |
|------|-------------|
| `airbnb_search` | Search listings by location, dates, guests |
| `airbnb_listing_details` | Full details for a specific listing |
| `airbnb_reviews` | Paginated reviews for a listing |
| `airbnb_price_calendar` | Price and availability calendar |
| `airbnb_host_profile` | Host profile (superhost, response rate, languages, bio) |
| `airbnb_neighborhood_stats` | Aggregated area stats (avg/median price, ratings, property types) |
| `airbnb_occupancy_estimate` | Occupancy rate, weekday/weekend prices, monthly breakdown |

## Dependencies
- `rmcp` 0.16 — Official MCP Rust SDK (schemars 1.0)
- `reqwest` — HTTP client with cookie support
- `scraper` — HTML parsing with CSS selectors
- `lru` — In-memory LRU cache
