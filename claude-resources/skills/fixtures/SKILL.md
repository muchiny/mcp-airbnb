---
name: fixtures
description: List and describe the HTML/JSON test fixtures in tests/fixtures/, and walk through how to capture a fresh fixture from a live Airbnb URL when a parser regression appears. Use when a scraper or GraphQL parser test fails unexpectedly, or when adding coverage for a new Airbnb response shape.
---

# /fixtures — Test fixtures walkthrough

Use this skill to understand the fixture layout in `tests/fixtures/` and to
capture a new fixture when a parser breaks on a real response shape that
isn't covered by the existing snapshots.

## Step 1 — List existing fixtures

```bash
ls -la tests/fixtures/ 2>&1
find tests/fixtures -type f -name '*.html' -o -name '*.json' 2>&1 | head -30
```

Report how many files are present and group them by category (search
pages, detail pages, review pages, calendar responses, host profiles).

## Step 2 — Map fixtures to parser tests

For each fixture file, find the test that loads it. The convention is:

```rust
// src/adapters/scraper/<kind>_parser.rs
#[test]
fn parse_<fixture_name>() {
    let html = include_str!("../../../tests/fixtures/<file>.html");
    let result = parse_<kind>(html).unwrap();
    assert!(...);
}
```

Run `rg 'include_str.*tests/fixtures' src/` to see the full mapping, or
just grep for the fixture filename to find its consumer(s).

## Step 3 — Capture a new fixture

When you see a parser test failing on a real-world response that the
current fixture doesn't cover, capture a fresh one:

1. **Find the exact URL** that's failing (from logs, `RUST_LOG=debug`, or
   by reproducing the request).

2. **Fetch the HTML** with the project's user agent (see `config.yaml`):

   ```bash
   curl -sSL \
        -H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36" \
        -H "Accept-Language: en-US,en;q=0.9" \
        'https://www.airbnb.com/rooms/<id>' \
        > tests/fixtures/rooms_<id>_$(date +%Y%m%d).html
   ```

   For GraphQL endpoints, you need the extracted API key — reproduce the
   request from `src/adapters/graphql/client.rs` (or capture via Chrome
   DevTools → Network → Copy as cURL).

3. **Minify or redact**: the raw Airbnb HTML is ~2MB per page. If you
   commit it as-is, the repo bloats fast. Two options:
   - Keep only the JSON island that the parser actually reads (hunt for
     `<script id="data-deferred-state-...">` or `__NEXT_DATA__`)
   - Gzip the full HTML: `gzip tests/fixtures/rooms_<id>.html` and update
     the test to read through `flate2::read::GzDecoder`

4. **Write the test** that loads the fixture and asserts on the fix:

   ```rust
   #[test]
   fn parse_regression_rooms_<id>() {
       let html = include_str!("../../../tests/fixtures/rooms_<id>_<date>.html");
       let result = parse_detail(html).expect("should parse");
       assert_eq!(result.price_per_night, 142.0); // the field that was broken
   }
   ```

5. **Commit** the fixture + test in the same commit as the parser fix so
   the regression story is atomic and bisectable.

## Notes

- Fixtures are public data (Airbnb listing pages are unauthenticated), but
  **redact personal info** before committing: host full names if unusual,
  phone numbers, email addresses, anything that looks like PII. The JSON
  island usually doesn't contain those, but double-check with `grep -iE
  '(phone|email|@)'` before `git add`.
- Do not capture fixtures from logged-in Airbnb sessions — the response
  shape differs from the public one and you'll train parsers on the wrong
  format.
- Never re-capture an existing fixture to "update" it — the old fixture
  exists precisely because some regression was fixed against it.
  Additive only: new filename, new test.
