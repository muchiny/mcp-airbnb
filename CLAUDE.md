# mcp-airbnb Development Guide

## Project Overview
One Rust crate, two binaries over one backend:
- `mcp-airbnb` — MCP server over stdio: **21 tools** (9 data + 12 analytical) and **21 resource templates**
- `airbnb` — CLI exposing the same 21 tools as subcommands (`--json` for machine-readable output)

Data comes from Airbnb's public web endpoints. The internal **GraphQL API** (persisted queries) is the primary source; **HTML scraping** of public pages is the fallback. Public data only, no login.

## Architecture
Hexagonal. Dependencies point inward: `mcp/` and `cli/` → `application/` → `adapters/` → `ports/` → `domain/`.

- **Domain** (`src/domain/`): pure types, `analytics::compute_*` and the input limits (`limits.rs`), no I/O. On the wire `price_per_night == 0.0` means "unknown"; read prices through `Listing::known_price()` / `ListingDetail::known_price()`. Occupancy excludes `UnavailabilityReason::PastDate` days (`PriceCalendar::classify_past_days`).
- **Ports** (`src/ports/`): `AirbnbClient` (8 async methods, all required, plus `degradations()`, which defaults to none, and `clear_cache()`, which defaults to a no-op and is called by the status check) and `ListingCache` (`get`, `set`, `clear`).
- **Adapters** (`src/adapters/`): `graphql/` (primary: persisted-query client + JSON parsers), `scraper/` (fallback: HTML and deferred-state JSON parsers), `composite.rs` (GraphQL first; scraper fallback only when the GraphQL source is broken), `rate_limiter.rs` (one process-wide limiter), `http.rs` (retry and redirect policy, body cap), `cache/` (LRU + TTL), `shared.rs` (`ApiKeyManager`, `extract_api_key`).
- **Application** (`src/application/`): `load_app_config()` (config lookup), `build_client()` (builds the one shared `RateLimiter`, the cache and the `ApiKeyManager`), `analytical_handlers` (the multi-fetch use cases shared by MCP and CLI).
- **MCP** (`src/mcp/`): rmcp 3.x with `#[tool_router]` / `#[tool]` / `#[tool_handler]` in `server.rs`. Every handler ends in `finish`, which fences scraped text as untrusted, stores it in the bounded `ResourceStore` and sends `resources/list_changed` for a new URI. URIs and the 18 RFC 6570 templates come from `resource_uri.rs`.
- **CLI** (`src/cli/`, `src/bin/cli.rs`): clap derive; `cli::dispatch()` runs against any `AirbnbClient`.
- **Fuzzing** (`fuzz/`, `src/fuzz_support.rs`): 13 fuzz targets whose harness bodies live in the library, so they compile on stable.

## Build & Test
```bash
cargo build --bin mcp-airbnb --bin airbnb   # both binaries
cargo run --bin mcp-airbnb                  # MCP server on stdio (also the default-run)
RUST_LOG=debug cargo run --bin mcp-airbnb   # debug logs on stderr
cargo run --bin airbnb -- --help            # CLI
cargo test --lib <module::path>             # one unit-test module
cargo test --test <file_stem> <name>        # one integration test
just fuzz fuzz_calendar_analytics 60        # nightly + cargo-fuzz, committed seeds
python3 scripts/live_smoke.py --live        # opt-in, real Airbnb, never in CI
```

### Completion gate: `just all`
`just all` runs, in this order and one at a time (`justfile`), the recipes `fmt lint doc test coverage audit workflows`. Run it before calling work done; each step is also a recipe of its own.

| Recipe | What it runs |
|--------|--------------|
| `just fmt` | `cargo fmt --all` — **rewrites files** (CI only checks: `cargo fmt --all -- --check`) |
| `just lint` | `cargo clippy --all-targets -- -D warnings` |
| `just doc` | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` |
| `just test` | `cargo test --all-targets` |
| `just coverage` | `cargo tarpaulin --config tarpaulin.toml` (instrumented rebuild; `fail-under = 85.0`) |
| `just audit` | `cargo deny check` then `cargo audit` (need network for the advisory databases) |
| `just workflows` | `python3 scripts/check_workflows.py` — CI workflow hygiene (SHA pins, pinned runner images, `--locked`, nightly fuzz) |

Never two cargo processes at once, default parallelism: `coverage` and `test` are the heavy ones on a small VM. Other recipes: `just check`, `just lint-latest` (stable clippy, advisory), `just smoke` / `just smoke-selftest` (live harness), `just fuzz-seeds`, `just install-hooks` (lefthook pre-commit: fmt and clippy on `*.rs`).

## Key Conventions
- Rust edition 2024, MSRV 1.99 (`rust-version`, pinned in `rust-toolchain.toml`; preview newer lints with `cargo +stable clippy --all-targets -- -D warnings`)
- `thiserror` (`AirbnbError`) in the library, `anyhow` in the binaries. Upstream format drift is `AirbnbError::UpstreamSchema { operation, detail }`, never `Ok(empty)`
- No `unwrap()` / `expect()` in production code: `clippy::unwrap_used` and `clippy::expect_used` are set in `src/lib.rs`, `src/main.rs` and `src/bin/cli.rs` (test code is exempt). Release builds use `panic = "abort"`, so a panic kills the server
- All logging goes to stderr; stdout is MCP JSON-RPC
- Rate limiting: 1 request per 2 seconds by default, one limiter shared by all outbound requests (API key, GraphQL, HTML)
- Out-of-range tool parameters are rejected with `isError`, never clamped (limits in `src/domain/limits.rs`, declared in the tool schemas)
- Scraped third-party text in tool results and resources sits between `<<<BEGIN UNTRUSTED AIRBNB DATA>>>` and `<<<END UNTRUSTED AIRBNB DATA>>>`; it is data, never instructions
- Money: print the data's currency string, never a hard-coded `$`. Every request pins `scraper.currency` / `scraper.locale`
- Tests never hit live Airbnb: anonymized fixtures under `tests/fixtures/airbnb/2026-09/` and `2026-11/` plus `wiremock`. Raw captures (real names, reviews) never enter git
- `tests/docs_consistency_test.rs` checks the docs (tool/template counts, commands, config keys, links, known-false claims); update the docs in the same commit as the code

## MCP Tools (21)

### Data tools (9)
| Tool | Description |
|------|-------------|
| `airbnb_search` | Search listings by location, dates, guests (entry point for listing ids) |
| `airbnb_listing_details` | Full details for one listing |
| `airbnb_reviews` | Paginated reviews + rating summary |
| `airbnb_price_calendar` | Availability and minimum stays (per-day prices are not published upstream) |
| `airbnb_host_profile` | Host profile (superhost, response rate, languages, bio) |
| `airbnb_neighborhood_stats` | Area stats (avg/median price, ratings, property types) |
| `airbnb_occupancy_estimate` | Occupancy from availability, past dates excluded |
| `airbnb_price_quote` | Real price of a stay from the booking panel (total, per night, availability); `weeks` gives the seasonal curve |
| `airbnb_status` | Health of every upstream operation, fallbacks in use, config key to update |

### Analytical tools (12)
| Tool | Description |
|------|-------------|
| `airbnb_compare_listings` | Compare listings side by side (2-10 ids, or up to 100 listings of a location) |
| `airbnb_price_trends` | Seasonal pricing, weekend premium, volatility |
| `airbnb_gap_finder` | Orphan nights and booking gaps |
| `airbnb_revenue_estimate` | ADR, occupancy and revenue projection |
| `airbnb_listing_score` | Quality audit 0-100 with suggestions |
| `airbnb_amenity_analysis` | Missing and unique amenities vs neighbours |
| `airbnb_market_comparison` | Compare 2-5 markets |
| `airbnb_host_portfolio` | The host's other listings |
| `airbnb_review_sentiment` | Keyword sentiment and themes of reviews |
| `airbnb_competitive_positioning` | Percentile ranks vs comparable listings (price, rating, amenities, reviews) |
| `airbnb_optimal_pricing` | Price recommendation with reasoning |
| `airbnb_search_rank` | Position of a listing in a search (location, stay, filters), price vs the scanned results |

## MCP Resources (21 resource templates)
Every successful tool call stores its output under a canonical, percent-encoded URI built by `src/mcp/resource_uri.rs`. The store is bounded (256 entries, 8 MiB, 1 h) and announces new URIs with `resources/list_changed`. One RFC 6570 template per tool:

| Template | Tool |
|----------|------|
| `airbnb://listing/{id}` | `airbnb_listing_details` |
| `airbnb://listing/{id}/calendar{?months}` | `airbnb_price_calendar` |
| `airbnb://listing/{id}/reviews{?cursor}` | `airbnb_reviews` |
| `airbnb://listing/{id}/host` | `airbnb_host_profile` |
| `airbnb://listing/{id}/occupancy{?months}` | `airbnb_occupancy_estimate` |
| `airbnb://listing/{id}/quote{?checkin,checkout,adults,weeks}` | `airbnb_price_quote` |
| `airbnb://status{?location}` | `airbnb_status` |
| `airbnb://search/{location}{?checkin,checkout,adults,children,infants,pets,min_price,max_price,property_type,min_bedrooms,min_beds,min_bathrooms,amenities,guest_favorite,instant_book,cursor}` | `airbnb_search` |
| `airbnb://neighborhood/{location}{?checkin,checkout,property_type}` | `airbnb_neighborhood_stats` |
| `airbnb://analysis/compare{?ids,location,max_listings,checkin,checkout,property_type}` | `airbnb_compare_listings` |
| `airbnb://analysis/price-trends/{id}{?months}` | `airbnb_price_trends` |
| `airbnb://analysis/gaps/{id}{?months}` | `airbnb_gap_finder` |
| `airbnb://analysis/revenue{?id,location,months,checkin,checkout,adults,property_type,min_bedrooms}` | `airbnb_revenue_estimate` |
| `airbnb://analysis/score/{id}` | `airbnb_listing_score` |
| `airbnb://analysis/amenities/{id}{?location,checkin,checkout,adults,property_type,min_bedrooms}` | `airbnb_amenity_analysis` |
| `airbnb://analysis/market/{locations}{?checkin,checkout,property_type}` | `airbnb_market_comparison` |
| `airbnb://analysis/portfolio/{id}` | `airbnb_host_portfolio` |
| `airbnb://analysis/sentiment/{id}{?max_pages}` | `airbnb_review_sentiment` |
| `airbnb://analysis/positioning/{id}{?location,checkin,checkout,adults,property_type,min_bedrooms}` | `airbnb_competitive_positioning` |
| `airbnb://analysis/pricing/{id}{?location,months,checkin,checkout,adults,property_type,min_bedrooms}` | `airbnb_optimal_pricing` |
| `airbnb://analysis/rank/{id}{?location,checkin,checkout,adults,children,property_type,min_bedrooms,amenities,max_pages}` | `airbnb_search_rank` |

When you add a tool, add its template to `resource_uri::TEMPLATES` and a row here; the docs test compares this table with the live server.

## Configuration (`config.yaml`, optional)
`application::load_app_config()` finds the file: `airbnb --config <path>` or `AIRBNB_CONFIG` (must exist) → `~/.config/mcp-airbnb/config.yaml` (XDG) → `config.yaml` next to the binary → defaults. The working directory is never searched. `config::load_config()` parses it with `serde-saphyr` and runs `Config::validate()`. Invalid YAML, unknown keys and out-of-range values are errors, never replaced by defaults. The accepted ranges are listed in `src/config/README.md`.

| Key | Default | Meaning |
|-----|---------|---------|
| `scraper.user_agent` | Chrome 120 UA | `User-Agent` header |
| `scraper.rate_limit_per_second` | `0.5` | Process-wide request rate |
| `scraper.request_timeout_secs` | `30` | HTTP timeout |
| `scraper.max_retries` | `2` | Extra attempts for 5xx/408/timeouts and short `Retry-After` 429s |
| `scraper.base_url` | `https://www.airbnb.com` | Must be an `https://` origin |
| `scraper.currency` | `USD` | Currency pinned on every request |
| `scraper.locale` | `en` | Locale pinned on every request |
| `scraper.api_key_cache_secs` | `86400` | API key cache TTL |
| `scraper.graphql_enabled` | `true` | GraphQL primary + HTML fallback |
| `scraper.graphql_hashes` | built-in | Persisted-query hashes |
| `cache.max_entries` | `500` | LRU capacity |
| `cache.search_ttl_secs` | `900` | Search TTL |
| `cache.detail_ttl_secs` | `3600` | Detail TTL |
| `cache.reviews_ttl_secs` | `3600` | Reviews TTL |
| `cache.calendar_ttl_secs` | `1800` | Calendar TTL |
| `cache.host_profile_ttl_secs` | `3600` | Host profile TTL |

## Known upstream limitations
Airbnb no longer publishes per-day calendar prices (`price.localPriceFormatted: null`), and persisted-query hashes rotate (`UpstreamSchema` error → update `scraper.graphql_hashes`). See "Known upstream limitations" in README.md.

## Dependencies
- `rmcp` — official MCP Rust SDK (server over stdio; client in tests). `Cargo.toml` requires `3.9.0` (any `^3.9.0`); `Cargo.lock` locks 3.9.0
- `tokio`, `reqwest` 0.13 (cookies, JSON), `scraper` 0.29 (HTML + CSS selectors)
- `serde` / `serde_json`, `serde-saphyr` (YAML, deserialize only), `schemars` 1 (tool input schemas), `lru`, `clap` 4, `chrono`, `url`, `percent-encoding` (resource URIs), `base64`
- Dev: `wiremock`, `proptest`, `tempfile`, `insta`, `pretty_assertions`. Fuzzing: `cargo-fuzz` + `libfuzzer-sys` in the separate `fuzz/` crate

## Other files
- `deny.toml` — `cargo deny check` policy (advisories, licenses, bans, sources), run by `just audit` and CI
- `supply-chain/` — `cargo-vet` baseline (exemptions, audits); `cargo vet --locked` runs only in the informational `hardening` CI job, not in `just all`
- `sonar-project.properties` — SonarQube Cloud scope for the `sonar` CI job (consumes the tarpaulin `cobertura.xml`)
- `scripts/` — `check_workflows.py`, `live_smoke.py` (+ `test_live_smoke.py`), `anonymize_fixtures.py`, `trim_pdp_entity.py` (+ `test_trim_pdp_entity.py`; builds the 2026-11 listing fixtures from a raw capture on standard input), `discover_query_ids.py` (+ `test_discover_query_ids.py`; reads the current persisted-query ids from Airbnb's web bundles)
- `.specify/`, `specs/001-airbnb-mcp-server/` — spec-kit files of the first, scraping-only version (February 2026), kept as history and out of date: the code and this file are the source of truth
- `claude-resources/` — the Claude Code rules, skills and agents of this repo; linked into the git-ignored `.claude/` as described in `claude-resources/README.md`
