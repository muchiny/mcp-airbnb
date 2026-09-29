---
name: fixtures
description: List and describe the anonymized JSON test fixtures in tests/fixtures/airbnb/, and walk through how to capture, anonymize and commit a fresh fixture from a live Airbnb URL when a parser regression appears. Use when a scraper or GraphQL parser test fails unexpectedly, or when adding coverage for a new Airbnb response shape.
---

# /fixtures — Test fixtures walkthrough

Use this skill to understand the fixture layout in `tests/fixtures/airbnb/`
and to capture a new fixture when a parser breaks on a real response shape
that isn't covered by the existing snapshots.

**Privacy rule (from `CLAUDE.md`): Raw captures (real names, reviews) never enter git.**
Every committed fixture is an anonymized JSON file; raw pages and responses
stay under `/var/tmp/`, outside the repo.

## Step 1 — List existing fixtures

```bash
ls -la tests/fixtures/airbnb/ 2>&1
find tests/fixtures/airbnb -type f -name '*.json' 2>&1 | head -30
```

Report how many files are present and group them by category (search,
listing detail, reviews, calendar, request bodies). Each month directory has
a `README.md` describing its captures and how they were anonymized.

## Step 2 — Map fixtures to parser tests

Unit tests load a fixture with `crate::test_helpers::fixture_json("<dir>/<file>.json")`
(relative to `tests/fixtures/airbnb/2026-09/`); HTML-scraper tests rebuild a
page around a JSON payload with `crate::test_helpers::niobe_page(...)`.
Integration tests use `include_str!("fixtures/airbnb/2026-09/<dir>/<file>.json")`.

Run `rg 'fixture_json|fixtures/airbnb' src/ tests/` to see the full mapping,
or grep for the fixture filename to find its consumer(s).

## Step 3 — Capture a new fixture

When you see a parser test failing on a real-world response that the
current fixture doesn't cover, capture a fresh one:

1. **Find the exact URL** that's failing (from logs, `RUST_LOG=debug`, or
   by reproducing the request).

2. **Fetch the raw capture outside the repo** with the project's user agent
   (see `config.yaml`):

   ```bash
   curl -sSL \
        -H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36" \
        -H "Accept-Language: en-US,en;q=0.9" \
        'https://www.airbnb.com/rooms/<id>' \
        > /var/tmp/rooms_<id>_$(date +%Y%m%d).html
   ```

   For GraphQL endpoints, you need the extracted API key — reproduce the
   request from `src/adapters/graphql/client.rs` (or capture via Chrome
   DevTools → Network → Copy response) and save the JSON under `/var/tmp/`
   too. The raw capture contains real names, review text, host bios and
   user photo URLs: never copy it into the repo as-is.

3. **Keep only the JSON the parser reads.** Never commit an HTML page. From
   an HTML capture, extract the embedded deferred-state JSON island
   (`<script id="data-deferred-state-0">`, or `__NEXT_DATA__` on older
   pages) into `/var/tmp/<name>.json`, and drop the parts the parser does
   not read.

4. **Anonymize it**, still under `/var/tmp/`:
   - with `scripts/anonymize_fixtures.py`: add the capture to its `FILES`
     map (and to `trim()` if it needs trimming), then run it as described in
     `tests/fixtures/airbnb/2026-09/README.md`. The script only processes
     the JSON files listed in `FILES`, reads them with `json.loads`, and
     exits 1 if any listed raw capture is missing from `--src`, so this
     path needs the complete raw capture set;
   - otherwise, by hand: replace people's names with `PersonN`, review text,
     host bios, host responses and reviewer locations with
     `Placeholder text N.`, user and review ids with `100000N`, session,
     share and trace ids with zero UUIDs, and every `a0.muscache.com` URL
     with `https://example.com/fixture-image/N.jpg`.

   Then check for leaks before saving it under
   `tests/fixtures/airbnb/$(date +%Y-%m)/`:

   ```bash
   grep -nE 'a0\.muscache\.com|/users/(show|profile)/[1-9]|@|\bphone\b' /var/tmp/<name>.json
   grep -nF '<a real name from the raw capture>' /var/tmp/<name>.json
   ```

   Both must print nothing. Add the new file to `COMMITTED_FIXTURES` in
   `src/test_helpers.rs` so `fixtures_are_anonymized` checks it, document
   it (source and anonymization) in the month directory's `README.md`, and
   regenerate the fuzz seeds with `python3 fuzz/make_seeds.py`.

5. **Write the test** that loads the fixture and asserts on the fix:

   ```rust
   #[test]
   fn parse_regression_rooms_<id>() {
       let payload = crate::test_helpers::fixture_json("<dir>/rooms_<id>_<date>.json");
       let html = crate::test_helpers::niobe_page("niobeClientData", &[("StaysPdpSections:{}", &payload)]);
       let result = parse_listing_detail(&html, "<id>", "https://www.airbnb.com").expect("should parse");
       assert_eq!(result.price_per_night, 142.0); // the field that was broken
   }
   ```

6. **Commit** the fixture + test in the same commit as the parser fix so
   the regression story is atomic and bisectable. Stage explicit paths
   only, and never the raw capture.

## Notes

- Raw captures (real names, reviews) never enter git. Listing pages are
  unauthenticated, but they carry third-party personal data (reviewer and
  host names, review text, bios, user photos): only the anonymized JSON is
  committed, and `cargo test --lib fixtures_are_anonymized` must pass.
- Do not capture fixtures from logged-in Airbnb sessions — the response
  shape differs from the public one and you'll train parsers on the wrong
  format.
- Never re-capture an existing fixture to "update" it — the old fixture
  exists precisely because some regression was fixed against it.
  Additive only: new filename, new test.
