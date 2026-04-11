# Scraping Conventions (`mcp-airbnb`)

Rules for everything under `src/adapters/scraper/` and
`src/adapters/graphql/`. Applies when adding a new scraper, fixing a
parser, or extending the composite client.

## Rate limiting is non-negotiable

Default: `1 request per 2 seconds` (`config.scraper.rate_limit_per_second = 0.5`).
This is not a suggestion — Airbnb will Cloudflare-ban a higher rate. The
rate limiter lives in `src/adapters/scraper/rate_limiter.rs` and is
enforced per-client. If you see `AirbnbError::RateLimited` in tests or
logs, **do not raise the limit** to make it go away — diagnose why the
request was made at all.

## Fixtures, not live traffic

Unit and integration tests **never hit live Airbnb**. They hit:

1. `tests/fixtures/*.html` / `*.json` — real HTML/JSON responses captured
   once, committed to the repo, used to drive parser tests deterministically
2. `wiremock` mocks — for full-stack HTTP tests that exercise the reqwest
   client path (see `tests/scraper_test.rs`)

When a new failure mode appears in production, capture a fresh fixture:

```bash
curl -s -H "User-Agent: <UA from config.yaml>" \
     'https://www.airbnb.com/rooms/<id>' \
     > tests/fixtures/rooms_<id>_<yyyymmdd>.html
```

Then write a parser test that loads it and asserts on the expected output.
Never skip this step — the parser lineage in `adapters/scraper/*_parser.rs`
is the most fragile part of the codebase and needs regression coverage.

## Composite client pattern

`CompositeClient` (in `src/adapters/composite.rs`) is the "strategy"
wrapper: it tries the GraphQL client first, falls back to the HTML scraper
on error. It also merges partial responses — if GraphQL returns a listing
detail without amenities but the scraper has them, the merge fills in the
gaps. See `detail_merge_*` tests in `composite.rs` for the merge rules.

When adding a new field to `ListingDetail`, `Listing`, `HostProfile`, etc.:
- Add the field to the domain type with `#[serde(default)]` if it's optional
- Implement extraction in BOTH `graphql::parsers::*` AND `scraper::*_parser`
- Add a merge rule in `composite::detail_merge_*` (if the field is optional)
- Write a test that asserts the merge does the right thing when one side
  is missing

## API key rotation

The scraper and GraphQL client both need Airbnb's public JS API key
(`api_key` in the request headers). It's extracted from Airbnb's JS
bundle on first request and cached for `config.scraper.api_key_cache_secs`
(default 1 hour). The cache lives in `ApiKeyManager` (`src/adapters/shared.rs`).
Both clients share a single `Arc<ApiKeyManager>` — do not create two.

When the key stops working, it's almost always because Airbnb rotated it;
the manager will re-extract on the next failure. If failures persist,
check that the extraction regex in `shared.rs::extract_api_key_from_html`
still matches the current JS bundle shape.

## Caching TTLs

| Data type   | Default TTL | Config field                        |
|-------------|-------------|--------------------------------------|
| Search      | 15 min      | `cache.search_ttl_secs` (900)        |
| Listing     | 1 hour      | `cache.detail_ttl_secs` (3600)       |
| Reviews     | 6 hours     | `cache.reviews_ttl_secs` (21600)     |
| Calendar    | 30 min      | `cache.calendar_ttl_secs` (1800)     |
| Neighborhood| 6 hours     | `cache.neighborhood_ttl_secs` (21600)|

The cache is an in-memory LRU (`src/adapters/cache/memory_cache.rs`),
bounded by `cache.max_entries` (default 500). Do not introduce a disk
cache without discussing: it changes the privacy profile of the tool.

## Legal and ethical boundaries

This project scrapes **public listing data only**:
- No login, no cookies-for-authentication
- No data about guests beyond public review text (which Airbnb displays to
  unauthenticated visitors)
- No attempt to bypass Cloudflare or WAF rules (the default UA is a
  plain browser string and the rate limit is conservative)
- No re-distribution of scraped data as a dataset

If a feature request crosses these lines, say no in the PR. The project's
positioning depends on staying clearly on the "public research tool" side.
