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

### Data Tools
| Tool | Description |
|------|-------------|
| `airbnb_search` | Search listings by location, dates, guests |
| `airbnb_listing_details` | Full details for a specific listing |
| `airbnb_reviews` | Paginated reviews for a listing |
| `airbnb_price_calendar` | Price and availability calendar |
| `airbnb_host_profile` | Host profile (superhost, response rate, languages, bio) |
| `airbnb_neighborhood_stats` | Aggregated area stats (avg/median price, ratings, property types) |
| `airbnb_occupancy_estimate` | Occupancy rate, weekday/weekend prices, monthly breakdown |

### Analytical Tools (compose data tools, no new scraping)
| Tool | Description |
|------|-------------|
| `airbnb_compare_listings` | Compare 2-100+ listings side-by-side (by IDs or location) |
| `airbnb_price_trends` | Seasonal pricing: peak/off-peak months, weekend premium, volatility |
| `airbnb_gap_finder` | Detect orphan nights and booking gaps with lost revenue estimate |
| `airbnb_revenue_estimate` | Project ADR, occupancy rate, monthly/annual revenue |
| `airbnb_listing_score` | Quality audit 0-100 with category scores and improvement suggestions |
| `airbnb_amenity_analysis` | Missing popular amenities vs neighborhood competition |
| `airbnb_market_comparison` | Compare 2-5 neighborhoods side-by-side |
| `airbnb_host_portfolio` | Analyze a host's full property portfolio |

### MCP Resources
Data fetched by tools is automatically cached as MCP resources (e.g. `airbnb://listing/{id}`, `airbnb://listing/{id}/calendar`, `airbnb://search/{location}`). Clients can reference previously fetched data without re-scraping.

## Dependencies
- `rmcp` 0.16 — Official MCP Rust SDK (schemars 1.0)
- `reqwest` — HTTP client with cookie support
- `scraper` — HTML parsing with CSS selectors
- `lru` — In-memory LRU cache
