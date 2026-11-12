---
name: mcp-smoke
description: Smoke-test the mcp-airbnb MCP server by building it, sending an `initialize` + `tools/list` JSON-RPC request via stdin, and verifying the response contains the expected 21 tools. Use when you've changed anything in src/mcp/, src/application/, or the AirbnbClient trait and want a quick end-to-end sanity check before running the full test suite.
---

# /mcp-smoke — MCP server smoke test

Use this skill to verify the `mcp-airbnb` binary still speaks a valid MCP
JSON-RPC handshake after changes. It is intentionally minimal: build +
initialize + tools/list, nothing more. For behavioural assertions, use
`cargo test` instead.

## Steps

1. **Build the binary** in debug mode (faster than release for smoke):

   ```bash
   cargo build --bin mcp-airbnb 2>&1 | tail -5
   ```

   If the build fails, stop and report the compile error.

2. **Feed a two-request JSON-RPC sequence** to the binary via stdin. The
   server reads line-delimited JSON-RPC over stdio:

   ```bash
   {
     printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}'
     printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
     printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
   } | timeout 5 ./target/debug/mcp-airbnb 2>/dev/null
   ```

3. **Verify the response**:
   - `id:1` — `result.serverInfo.name` should be `"mcp-airbnb"`
   - `id:2` — `result.tools` should be an array of length **21** (the output
     has one JSON object per line, one per reply), and should contain `airbnb_search`, `airbnb_listing_details`,
     `airbnb_optimal_pricing` (spot-check three)

4. **Report** a one-line status:
   - ✅ `mcp-airbnb smoke passed: 21 tools listed, server name OK`
   - ❌ `mcp-airbnb smoke FAILED: <reason>` — include the raw response for
     whichever request failed

## Notes

- Stderr is expected to contain tracing logs (`Starting mcp-airbnb server`,
  `GraphQL mode enabled`, …). Those are noise, not failures — filter with
  `2>/dev/null` as shown above.
- The brace group closes stdin once the three lines are written, and the
  server exits on that EOF (about 0.2 s, exit code 0), so `timeout 5` is only
  a safety net. If the reply to `id:2` is ever missing, add a short `sleep 1`
  as the last command of the brace group so stdin stays open until the
  server has answered.
- If the binary doesn't exist yet, the skill runs `cargo build --bin mcp-airbnb`
  first — do not skip this step even if you "just built".
- This skill assumes you are in the `mcp-airbnb` repo root. If `cargo build`
  can't find `Cargo.toml`, print `not in an mcp-airbnb project` and stop.

## Live behaviour

This skill only checks the handshake. To exercise all 21 tools against the
real site (opt-in, rate-limited, never in CI):

```bash
cargo build --release --bin mcp-airbnb
python3 scripts/live_smoke.py --live --location "Lyon, France"
```
