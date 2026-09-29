# Scraping Conventions (`mcp-airbnb`)

Rules for everything under `src/adapters/scraper/` and
`src/adapters/graphql/`. Applies when adding a new scraper, fixing a
parser, or extending the composite client.

## Rate limiting is non-negotiable

Default: `1 request per 2 seconds` (`config.scraper.rate_limit_per_second = 0.5`).
This is not a suggestion: Airbnb blocks higher rates. The rate limiter
lives in `src/adapters/rate_limiter.rs`. ONE instance is built in
`application::build_client` and shared by the GraphQL client, the HTML
scraper and `ApiKeyManager`, so the limit is the total rate. Never create a
second limiter. Every attempt, retries included, goes through
`adapters::http::send_with_retries`: the GraphQL client calls it directly and
classifies the final response itself, while the HTML scraper and the API-key
fetch use its 2xx-only wrapper `send_with_policy`. If you see `AirbnbError::RateLimited`
in tests or logs, **do not raise the limit** to make it go away — diagnose
why the request was made at all.

## Fixtures, not live traffic

Unit and integration tests **never hit live Airbnb**. They use:

1. `tests/fixtures/airbnb/<yyyy-mm>/` — real JSON responses captured once and
   **anonymized before commit** with `scripts/anonymize_fixtures.py`:
   reviewer and host names, review text, host bios and user photo URLs are
   replaced by placeholders. Unit tests load them with
   `test_helpers::fixture_json("<file>")`; integration tests use
   `include_str!`.
2. `wiremock` mocks — for full-stack HTTP tests of the reqwest client paths
   (see `tests/scraper_test.rs` and `tests/graphql_test.rs`).

When a new failure mode appears in production, capture a fresh payload
(browser devtools → Network → copy response, or the curl below), anonymize
it, commit it under a new `tests/fixtures/airbnb/<yyyy-mm>/` directory, write
a parser test that loads it, and regenerate the fuzz seeds:

```bash
curl -s -H "User-Agent: <UA from config.yaml>" \
     'https://www.airbnb.com/rooms/<id>' \
     > /var/tmp/rooms_<id>.html        # the raw capture stays OUT of the repo
# anonymize it, then save it under tests/fixtures/airbnb/$(date +%Y-%m)/
python3 fuzz/make_seeds.py            # fuzz seeds are derived from the fixtures
```

Raw captures contain third-party personal data and never enter git. To see
what the live site does end to end, run the opt-in
`python3 scripts/live_smoke.py --live` (never in CI).

## Upstream drift is an error, not an empty result

When a GraphQL response carries an `errors` array, a persisted query answers
HTTP 422, or the expected response root is missing, the adapter returns
`AirbnbError::UpstreamSchema { operation, detail }`. HTML fallbacks do the
same when a page has no recognisable data. Never map "could not find the
data" to `Ok(empty)`: that hides a rotated persisted-query hash behind "no
results".

## One document, one parser

The listing page embeds the same `StaysPdpSections` payload as the GraphQL
API, so the HTML scraper parses it with `graphql::parsers::{detail, host}`,
and search cards of both sources go through `adapters::stay_search`. Add an
HTML-only extraction path only for data that exists only in the HTML.

## Composite client pattern

`CompositeClient` (in `src/adapters/composite.rs`) tries the GraphQL client
first and falls back to the HTML scraper only when the GraphQL source is
broken (`composite::should_fall_back`: `Http`, `UpstreamStatus`, `Parse`,
`UpstreamSchema`, `Json`). `RateLimited`, `InvalidParams` and
`ListingNotFound` are returned as-is. When both fail it returns
`AllSourcesFailed { primary, fallback }`. After a GraphQL detail success,
the scraper is called only when name, location, description, amenities or
photos are missing; the `detail_merge_*` tests in `composite.rs` pin the
merge rules.

When adding a new field to `ListingDetail`, `Listing`, `HostProfile`, etc.:
- Add the field to the domain type with `#[serde(default)]` if it's optional
- Extract it in `graphql::parsers::*` (the scraper reuses those parsers on
  the embedded payload); add an HTML-only path only if the data is HTML-only
- If a missing value should trigger the scraper merge, extend the merge in
  `composite.rs` and add a `detail_merge_*` test for the case where one side
  is missing

## API key

The GraphQL client needs Airbnb's public web API key (`X-Airbnb-Api-Key`).
`ApiKeyManager` (`src/adapters/shared.rs`) extracts it from the homepage
with `shared::extract_api_key` (marker `"api_config":{"key":"`), caches it
for `config.scraper.api_key_cache_secs` (default 86400 s = 24 hours), and
fetches it single-flight through the shared rate limiter. All clients share
a single `Arc<ApiKeyManager>`; do not create two.

When Airbnb rejects the key (HTTP 401/403 on a GraphQL call), the client
calls `ApiKeyManager::invalidate` and retries once with a freshly extracted
key. A failed extraction is remembered for 2 minutes, so a broken homepage
is not re-downloaded on every call. If failures persist, check that
`shared.rs::extract_api_key` still matches the homepage.

## Currency and locale

Every request path (GraphQL GET, GraphQL POST, HTML GET) sends
`config.scraper.currency` and `config.scraper.locale` (`currency=`, `locale=`
and `Accept-Language`). Parsers never invent `$`: an amount without a label
gets the pinned currency's symbol. Display code prints the data's currency
string.

## Caching TTLs

| Data type    | Default TTL | Config field                          |
|--------------|-------------|---------------------------------------|
| Search       | 15 min      | `cache.search_ttl_secs` (900)         |
| Listing      | 1 hour      | `cache.detail_ttl_secs` (3600)        |
| Reviews      | 1 hour      | `cache.reviews_ttl_secs` (3600)       |
| Calendar     | 30 min      | `cache.calendar_ttl_secs` (1800)      |
| Host profile | 1 hour      | `cache.host_profile_ttl_secs` (3600)  |

The cache is an in-memory LRU (`src/adapters/cache/memory_cache.rs`),
bounded by `cache.max_entries` (default 500). Errors, including
`UpstreamSchema`, are never cached. Do not introduce a disk cache without
discussing: it changes the privacy profile of the tool.

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
