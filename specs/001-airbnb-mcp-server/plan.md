# Implementation Plan: Airbnb MCP Server

**Branch**: `001-airbnb-mcp-server` | **Date**: 2026-02-23 | **Spec**: spec.md

## Summary

Build an MCP server in Rust using `rmcp` 0.16 that enables AI assistants to search and browse Airbnb listings via web scraping of public pages, following hexagonal architecture patterns.

## Technical Context

**Language/Version**: Rust 1.93, edition 2024
**Primary Dependencies**: rmcp 0.16, reqwest, scraper, tokio, serde, lru
**Storage**: In-memory LRU cache
**Testing**: cargo test, wiremock, insta
**Target Platform**: Linux/macOS CLI (stdio MCP server)
**Project Type**: MCP server binary

## Architecture

### Hexagonal Layers

```
domain/ (pure types, no I/O)
  ├── listing.rs      → Listing, ListingDetail, SearchResult
  ├── review.rs       → Review, ReviewsSummary, ReviewsPage
  ├── calendar.rs     → PriceCalendar, CalendarDay
  └── search_params.rs → SearchParams validation

ports/ (trait definitions)
  ├── airbnb_client.rs → AirbnbClient trait
  └── cache.rs         → ListingCache trait

adapters/ (implementations)
  ├── scraper/
  │   ├── client.rs          → HTTP client (reqwest + AirbnbClient impl)
  │   ├── search_parser.rs   → HTML/JSON → SearchResult
  │   ├── detail_parser.rs   → HTML/JSON → ListingDetail
  │   ├── review_parser.rs   → HTML/JSON → ReviewsPage
  │   ├── calendar_parser.rs → HTML/JSON → PriceCalendar
  │   └── rate_limiter.rs    → Token bucket rate limiter
  └── cache/
      └── memory_cache.rs    → LRU cache with TTL

mcp/ (protocol layer)
  └── server.rs → AirbnbMcpServer with #[tool_router]/#[tool]/#[tool_handler]

config/ (configuration)
  ├── mod.rs   → YAML config loading
  └── types.rs → Config, ScraperConfig, CacheConfig
```

### Data Access Strategy

1. Build URL from search parameters
2. Fetch HTML via reqwest with configurable user-agent
3. Extract `__NEXT_DATA__` JSON from `<script>` tag (primary strategy)
4. Fall back to CSS selectors if JSON unavailable
5. Cache parsed results with configurable TTLs

## Constitution Check

- [x] Hexagonal architecture enforced
- [x] Pure domain layer (no I/O)
- [x] Rate limiting and robots.txt respect
- [x] Inline unit tests + integration test structure
- [x] rmcp official SDK with macros
- [x] English for all code and docs
