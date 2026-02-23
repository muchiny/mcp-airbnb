# mcp-airbnb Constitution

## Core Principles

### I. Hexagonal Architecture (Strict)
All code follows domain/ports/adapters separation. The dependency arrow always points inward: adapters → ports → domain. No direct imports between adapters. Dependency injection via `Arc<dyn Trait>`.

### II. Pure Domain Layer (NON-NEGOTIABLE)
The `domain/` module has ZERO external I/O dependencies. Allowed: `serde`, `thiserror`, `chrono`, `schemars`. Forbidden: `reqwest`, `tokio::fs`, `scraper`, `std::net`. Domain errors use `thiserror`.

### III. Web Scraping Ethics
Respect `robots.txt` by default. Rate limiting enforced (configurable, default 0.5 req/s). Configurable user-agent. Public data only — no authentication bypass. Clear error messages when blocked.

### IV. Test Discipline
- Inline `#[cfg(test)]` for unit tests in each module
- Integration tests with `wiremock` in `tests/integration/`
- Snapshot tests with `insta` for parser outputs
- HTML fixtures in `tests/fixtures/` as source of truth
- `.unwrap()` forbidden in production code (tests only)
- Assertions with `pretty_assertions`

### V. MCP Protocol Compliance
Use `rmcp` official SDK (v0.16+) with `#[tool_router]`/`#[tool]`/`#[tool_handler]` macros. All logging to stderr (stdout reserved for MCP JSON-RPC). Server implements `ServerHandler` trait.

### VI. English for Code & Docs
All code, comments, documentation, error messages, and commit messages in English.

### VII. Simplicity & YAGNI
No over-engineering. No premature abstractions. Exhaustive pattern matching. Complexity must be justified. Start simple, iterate.

## Technical Constraints

- **Language**: Rust, edition 2024, minimum version 1.93
- **Runtime**: Tokio (multi-thread)
- **Error handling**: `thiserror` (domain), `anyhow` (application)
- **Serialization**: `serde` + `serde_json` + `serde_yaml`
- **HTTP**: `reqwest` with cookie support
- **HTML parsing**: `scraper` crate (CSS selectors)
- **Cache**: In-memory LRU (`lru` crate)
- **Linting**: `clippy` pedantic, `unsafe_code = "forbid"`

## Development Workflow

1. `cargo build` — must compile without errors
2. `cargo test` — all tests must pass
3. `cargo clippy` — zero warnings
4. `cargo fmt` — code must be formatted
5. Manual test via Claude Code MCP integration

## Governance

This constitution supersedes all other practices. Amendments require documentation and justification. Use CLAUDE.md for runtime development guidance.

**Version**: 1.0.0 | **Ratified**: 2026-02-23 | **Last Amended**: 2026-02-23
