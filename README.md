# 🏠 mcp-airbnb

[![Rust](https://img.shields.io/badge/Rust-1.93%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![MCP](https://img.shields.io/badge/MCP-rmcp%200.16-blue)](https://modelcontextprotocol.io/)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)

> **Model Context Protocol server** that enables AI assistants to search and browse Airbnb listings via a dual data source: **GraphQL API** (primary) with **HTML scraping** fallback.

## ✨ Features

- 🔍 **Search listings** by location, dates, guests, price range, and property type
- 📋 **Listing details** with description, amenities, house rules, photos, and host info
- ⭐ **Reviews** with aggregate ratings and individual comments, paginated
- 📅 **Price calendar** with daily prices, availability, and minimum night requirements
- 👤 **Host profiles** with superhost status, response rate, languages, and bio
- 📊 **Neighborhood stats** with average/median prices, ratings, and property type distribution
- 📈 **Occupancy estimates** with weekday/weekend pricing and monthly breakdown
- 🔗 **Dual data source** — GraphQL API (fast, structured) + HTML scraper (fallback)
- 💾 **In-memory LRU cache** with configurable TTLs per tool
- ⏱️ **Rate limiting** to respect Airbnb (default: 1 request per 2 seconds)
- 🏗️ **Hexagonal architecture** — clean separation of domain, ports, and adapters

## 🏗️ Architecture

```mermaid
graph TB
    subgraph External["🌐 External"]
        AI["🤖 AI Assistant"]
        AB["🌍 Airbnb"]
    end

    subgraph MCP["📡 MCP Protocol Layer"]
        Server["AirbnbMcpServer<br/>rmcp 0.16 · stdio · 7 tools"]
    end

    subgraph Core["💎 Domain & Ports"]
        Domain["Domain Types<br/>Listing · Review · Calendar<br/>HostProfile · NeighborhoodStats"]
        Ports["Trait Boundaries<br/>AirbnbClient · ListingCache"]
    end

    subgraph Infra["⚡ Adapters"]
        Composite["🔀 CompositeClient<br/>GraphQL + Scraper fallback"]
        GQL["🔗 GraphQL Client<br/>Persisted queries"]
        Scraper["🕷️ HTML Scraper<br/>reqwest + parsing"]
        Cache["💾 Memory Cache<br/>LRU with TTL"]
        Shared["🔑 ApiKeyManager<br/>Auto-fetched key"]
    end

    AI <-->|"JSON-RPC<br/>over stdio"| Server
    Server --> Ports
    Ports --> Domain
    Composite -.->|"implements<br/>AirbnbClient"| Ports
    Cache -.->|"implements<br/>ListingCache"| Ports
    Composite --> GQL
    Composite --> Scraper
    GQL --> Shared
    Scraper --> Shared
    GQL -->|"GraphQL API"| AB
    Scraper -->|"HTTP GET"| AB
```

## 🔧 MCP Tools

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| 🔍 `airbnb_search` | Search listings by location, dates, and guests | `location` (required), `checkin`, `checkout`, `adults`, `min_price`, `max_price`, `property_type` |
| 📋 `airbnb_listing_details` | Full details for a specific listing | `id` |
| ⭐ `airbnb_reviews` | Paginated reviews with ratings summary | `id`, `cursor` |
| 📅 `airbnb_price_calendar` | Price and availability calendar | `id`, `months` (1–12, default: 3) |
| 👤 `airbnb_host_profile` | Host profile with superhost status and bio | `id` |
| 📊 `airbnb_neighborhood_stats` | Aggregated area statistics | `location`, `checkin`, `checkout`, `property_type` |
| 📈 `airbnb_occupancy_estimate` | Occupancy rate and pricing breakdown | `id`, `months` (1–12, default: 3) |

## 🚀 Quick Start

### Prerequisites

- **Rust 1.93+** (stable) — install via [rustup](https://rustup.rs/)

### Build & Run

```bash
# Build
cargo build --release

# Run the MCP server (stdio transport)
cargo run

# Run with debug logging (logs go to stderr)
RUST_LOG=debug cargo run
```

### Integration with Claude Desktop

Add to your Claude Desktop config (`~/.config/claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "airbnb": {
      "command": "/path/to/mcp-airbnb"
    }
  }
}
```

### Integration with Claude Code

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "mcp-airbnb": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/path/to/mcp-airbnb/Cargo.toml"]
    }
  }
}
```

## ⚙️ Configuration

All settings live in `config.yaml` (optional — sensible defaults are provided):

| Section | Field | Default | Description |
|---------|-------|---------|-------------|
| `scraper` | `rate_limit_per_second` | `0.5` | Max requests/s (0.5 = 1 req per 2s) |
| `scraper` | `request_timeout_secs` | `30` | HTTP timeout in seconds |
| `scraper` | `max_retries` | `2` | Retry count on failure |
| `scraper` | `base_url` | `https://www.airbnb.com` | Airbnb base URL |
| `scraper` | `graphql_enabled` | `true` | Enable GraphQL API (primary data source) |
| `scraper` | `api_key_cache_secs` | `86400` | API key cache TTL (24 hours) |
| `scraper` | `graphql_hashes` | *(built-in)* | Persisted query hashes for GraphQL operations |
| `cache` | `max_entries` | `500` | LRU cache capacity |
| `cache` | `search_ttl_secs` | `900` | Search cache TTL (15 min) |
| `cache` | `detail_ttl_secs` | `3600` | Detail cache TTL (1 hour) |
| `cache` | `reviews_ttl_secs` | `3600` | Reviews cache TTL (1 hour) |
| `cache` | `calendar_ttl_secs` | `1800` | Calendar cache TTL (30 min) |

> See [src/config/README.md](src/config/README.md) for the full configuration reference.

## 📁 Project Structure

```
mcp-airbnb/
├── src/
│   ├── domain/              # 💎 Pure types — Listing, Review, Calendar, Analytics
│   ├── ports/               # 🔌 Traits — AirbnbClient, ListingCache
│   ├── adapters/
│   │   ├── graphql/         # 🔗 GraphQL API client (primary)
│   │   │   ├── client.rs    #    Persisted queries, all 7 methods
│   │   │   └── parsers/     #    JSON → domain type parsers
│   │   ├── scraper/         # 🕷️ HTML scraper (fallback)
│   │   ├── cache/           # 💾 In-memory LRU cache
│   │   ├── composite.rs     # 🔀 GraphQL + Scraper with auto-fallback
│   │   └── shared.rs        # 🔑 ApiKeyManager (shared auth)
│   ├── mcp/                 # 📡 MCP server (rmcp 0.16, stdio, 7 tools)
│   ├── config/              # ⚙️ YAML configuration
│   ├── error.rs             # ❌ Error types (thiserror)
│   ├── lib.rs               # Module re-exports
│   └── main.rs              # 🚀 Entrypoint & DI wiring
├── tests/                   # 🧪 Integration tests + fixtures
├── config.yaml              # Runtime configuration
├── Cargo.toml               # Rust manifest
└── CLAUDE.md                # Development guide
```

> See [src/README.md](src/README.md) for the detailed architecture breakdown.

## 🔄 Request Flow

```mermaid
sequenceDiagram
    participant AI as 🤖 AI Assistant
    participant MCP as 📡 MCP Server
    participant Composite as 🔀 Composite
    participant Cache as 💾 Cache
    participant GQL as 🔗 GraphQL
    participant Scraper as 🕷️ Scraper
    participant AB as 🌍 Airbnb

    AI->>MCP: tool call (e.g. airbnb_search)
    MCP->>Composite: AirbnbClient method
    Composite->>Cache: Check cache
    alt Cache hit
        Cache-->>Composite: Cached result
    else Cache miss
        Composite->>GQL: Try GraphQL first
        GQL->>AB: GraphQL API request
        alt GraphQL OK
            AB-->>GQL: JSON response
            GQL-->>Composite: Parsed result
        else GraphQL fails
            Composite->>Scraper: Fallback to HTML
            Scraper->>AB: HTTP GET
            AB-->>Scraper: HTML response
            Scraper-->>Composite: Parsed result
        end
        Composite->>Cache: Store with TTL
    end
    Composite-->>MCP: Domain result
    MCP-->>AI: CallToolResult (formatted text)
```

## 🧪 Testing

```bash
cargo test                     # 🧪 Run all tests
cargo test --test mcp_server   # 📡 MCP tests only
cargo test --test scraper      # 🕷️ Scraper tests only
cargo clippy                   # 🔍 Lint
cargo fmt --check              # ✅ Check formatting
```

> See [tests/README.md](tests/README.md) for the test architecture and mock infrastructure.

## 📄 License

MIT
