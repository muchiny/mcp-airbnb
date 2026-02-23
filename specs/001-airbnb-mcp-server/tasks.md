# Tasks: Airbnb MCP Server

**Branch**: `001-airbnb-mcp-server` | **Date**: 2026-02-23

## Phase 1: Foundation [DONE]

- [x] T-001: Initialize cargo project with `cargo init mcp-airbnb`
- [x] T-002: Set up directory structure (domain/ports/adapters/mcp/config)
- [x] T-003: Write Cargo.toml with all dependencies (rmcp 0.16, schemars 1.0)
- [x] T-004: Write rust-toolchain.toml, config.yaml, .mcp.json, CLAUDE.md

## Phase 2: Domain & Ports [DONE]

- [x] T-005: Implement domain types (Listing, ListingDetail, SearchResult)
- [x] T-006: Implement domain types (Review, ReviewsSummary, ReviewsPage)
- [x] T-007: Implement domain types (PriceCalendar, CalendarDay)
- [x] T-008: Implement SearchParams with validation
- [x] T-009: Implement Display traits for all domain types
- [x] T-010: Write error.rs with AirbnbError enum (thiserror)
- [x] T-011: Define AirbnbClient trait (ports/airbnb_client.rs)
- [x] T-012: Define ListingCache trait (ports/cache.rs)

## Phase 3: Adapters [DONE]

- [x] T-013: Implement RateLimiter (token bucket)
- [x] T-014: Implement search_parser (__NEXT_DATA__ + CSS fallback)
- [x] T-015: Implement detail_parser
- [x] T-016: Implement review_parser
- [x] T-017: Implement calendar_parser
- [x] T-018: Implement AirbnbScraper (HTTP client, AirbnbClient impl)
- [x] T-019: Implement MemoryCache (LRU with TTL)
- [x] T-020: Implement config loading (YAML)

## Phase 4: MCP Server [DONE]

- [x] T-021: Implement AirbnbMcpServer struct with ToolRouter
- [x] T-022: Implement airbnb_search tool
- [x] T-023: Implement airbnb_listing_details tool
- [x] T-024: Implement airbnb_reviews tool
- [x] T-025: Implement airbnb_price_calendar tool
- [x] T-026: Implement ServerHandler with get_info()
- [x] T-027: Write main.rs entry point (tracing, config, stdio serve)

## Phase 5: Quality [DONE]

- [x] T-028: Unit tests for SearchParams validation
- [x] T-029: Unit tests for parsers (search, detail, review, calendar)
- [x] T-030: Unit tests for MemoryCache
- [x] T-031: Unit tests for RateLimiter
- [x] T-032: Unit tests for URL building
- [x] T-033: Fix all clippy warnings (0 warnings)
- [x] T-034: Format code with cargo fmt

## Phase 6: Spec-Kit [DONE]

- [x] T-035: Initialize spec-kit with `specify init`
- [x] T-036: Write constitution
- [x] T-037: Write specification
- [x] T-038: Write implementation plan
- [x] T-039: Write task breakdown (this file)

## Phase 7: Future Work [TODO]

- [ ] T-040: Integration tests with wiremock (mock Airbnb responses)
- [ ] T-041: Snapshot tests with insta (parser outputs)
- [ ] T-042: HTML fixture files for integration tests
- [ ] T-043: Manual test via Claude Code MCP integration
- [ ] T-044: Release build optimization and binary size
