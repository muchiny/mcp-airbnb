# 🧪 Tests

Integration and unit tests for the mcp-airbnb server.

## 📂 Files

| File | Scope | Description |
|------|-------|-------------|
| `mcp_server_test.rs` | 📡 MCP layer | Tests MCP server: tool registration, instructions, capabilities |
| `scraper_test.rs` | 🕷️ Scraper | Tests HTML parsing and scraping logic |
| `fixtures/` | 📁 Test data | HTML fixtures for parser tests |

## 🏛️ Test Architecture

```mermaid
flowchart TD
    Tests["🧪 Integration Tests"]
    Tests --> MockClient["🎭 MockAirbnbClient<br/>(from test_helpers.rs)"]
    MockClient --> Server["📡 AirbnbMcpServer"]
    Server --> Tools["🔧 15 Tool methods"]
```

> There are also 40+ inline unit tests in `src/mcp/server.rs` that test all 15 tools (7 data + 8 analytical) with mock clients.

## 🎭 Mock Infrastructure

Tests use `MockAirbnbClient` from `src/test_helpers.rs`, which implements `AirbnbClient` with configurable behavior via closures:

- `.with_search(|params| ...)` — 🔍 Mock search results
- `.with_detail(|id| ...)` — 📋 Mock listing details
- `.with_reviews(|id, cursor| ...)` — ⭐ Mock reviews
- `.with_calendar(|id, months| ...)` — 📅 Mock calendar
- `.with_host_profile(|id| ...)` — 👤 Mock host profiles
- `.with_neighborhood(|params| ...)` — 📊 Mock neighborhood stats
- `.with_occupancy(|id, months| ...)` — 📈 Mock occupancy estimates

## ▶️ Running Tests

```bash
cargo test                     # 🧪 Run all tests
cargo test --test mcp_server   # 📡 MCP tests only
cargo test --test scraper      # 🕷️ Scraper tests only
cargo test -- --nocapture      # 📝 Show output
```
