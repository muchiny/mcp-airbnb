---
name: cli-demo
description: Run the `airbnb` CLI binary through a short demonstration sequence — help, search, listing details in JSON, and a multi-fetch analytical command — to verify that the CLI wires through to the shared AirbnbClient and renders output correctly. Use after changing anything under src/cli/, src/application/, or the Display impls on domain types.
---

# /cli-demo — Airbnb CLI demonstration

Use this skill to exercise the `airbnb` binary through representative
commands. It covers:

- Help rendering (clap)
- Text mode output (Display impls)
- JSON mode output (serde_json)
- A multi-fetch analytical command (application layer orchestration)

## Steps

1. **Build the binary**:

   ```bash
   cargo build --bin airbnb 2>&1 | tail -5
   ```

2. **Run `--help`** and verify all 18 subcommands are listed:

   ```bash
   ./target/debug/airbnb --help
   ```

   Expected commands (count: 18):
   `search, listing, reviews, calendar, host, neighborhood, occupancy,
   compare, price-trends, gap-finder, revenue, listing-score, amenities,
   market-comparison, host-portfolio, review-sentiment, competitive,
   optimal-pricing`.

3. **Try a search** (real HTTP, uses the scraper's rate limit of 1 req / 2s —
   this **will** hit Airbnb, so only run it ad-hoc, not in CI):

   ```bash
   ./target/debug/airbnb search "Paris, France" --adults 2 --max-price 200 2>&1 | head -20
   ```

   Expected: human-readable output starting with `Found N listings` and
   listing at least one property. If it fails with a Cloudflare-style
   error, the scraper or API key cache is stale — see
   `claude-resources/rules/scraping-conventions.md` § API key rotation.

4. **JSON mode on a listing detail**:

   ```bash
   ./target/debug/airbnb --json listing 12345678 2>&1 | tail -30
   ```

   Pipe through `jq .` if available to verify valid JSON. The response
   should have `id`, `name`, `location`, `amenities` keys.

5. **A multi-fetch analytical command** (exercises
   `application::analytical_handlers`):

   ```bash
   ./target/debug/airbnb --json optimal-pricing 12345678 --months 6 2>&1 | tail -40
   ```

   Expected JSON with `listing_id`, `recommended_price`, `reasoning` keys.

6. **Report** which steps passed and which failed. If step 3/4/5 fail due
   to network issues (not compile/parse issues), report them as "network"
   rather than "broken" — the CLI is working, Airbnb isn't reachable.

## Notes

- Never run this in a CI job — the real scraping is rate-limited and
  flaky by design. For CI, use the inline-mock tests in `tests/cli_test.rs`
  via `cargo test --test cli_test`.
- If you only want to verify wiring without hitting the network, run
  steps 1, 2, then `cargo test --test cli_test` instead of steps 3–5.
- The listing ID `12345678` in the examples is a placeholder — substitute
  a real ID if you want a meaningful response.
