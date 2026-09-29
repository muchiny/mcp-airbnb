# MCP Server Conventions (`mcp-airbnb`)

Rules specific to the MCP protocol layer in `src/mcp/`. Applies when adding,
modifying, or debugging MCP tools.

## rmcp 1.x macros

Tools are declared via three attribute macros from `rmcp`:

```rust
#[tool_router]                  // on the `impl AirbnbMcpServer` block
#[tool(name = "...", description = "...", annotations(...))]
                                // on each async tool method
#[tool_handler]                 // on the `impl ServerHandler for AirbnbMcpServer`
```

See `src/mcp/server.rs` for 18 existing examples. Do not roll your own
JSON-RPC dispatch — always go through the macros.

## Tool handler shape

A tool handler is a **thin wrapper**. It must:

1. Take `Parameters(params): Parameters<MyToolParams>`.
2. Check every input with `check_input!(...)`: `validate_listing_id`, `limits::months`, `limits::count_in_range`, `SearchParams::validate`, … A failed check returns `isError` with the limit in the text. Nothing is clamped and nothing is fetched.
3. Data tools call `self.client.<method>(...)`. Analytical tools call the `application::analytical_handlers::run_*` function that the CLI also calls. Never orchestrate fetches or call `domain::analytics::compute_*` in `src/mcp/` (`tests/parity_test.rs` checks this).
4. Render with the result's `Display` and end with `Ok(self.finish(resource_uri::<kind>(…), name, text).await)`. `finish` fences the text as untrusted, stores it, and sends `resources/list_changed` when the URI is new.

## Parameter structs

- Live in `src/mcp/server.rs` near the top (or in the handler method if
  single-use)
- Must derive `Debug, serde::Deserialize, schemars::JsonSchema`
- Doc-comments on each field become the JSON schema `description` — write
  them as if the LLM will read them (it will)
- Optional fields: `Option<T>` with no default
- Ranges: declare them with `#[schemars(range(min = limits::X, max = limits::Y))]` / `#[schemars(length(...))]` / `#[schemars(pattern(LISTING_ID_PATTERN))]`, using the constants in `src/domain/limits.rs`, and enforce the same constants at runtime. Never `.clamp()`. Every parameter struct carries `#[serde(deny_unknown_fields)]`.

## Resource storage

Every successful tool result is stored through `finish`. URIs come only from the builders in `src/mcp/resource_uri.rs` (one RFC 6570 template per tool, 18 in total; every value percent-encoded; every result-changing input in the URI). `ResourceStore` is a bounded LRU: 256 entries, 8 MiB, 1 h TTL, URIs ≤ 16 KiB. `resources/list` returns pages of 100. Add a template to `TEMPLATES` whenever you add a tool.

## Error handling

Return an error **inside** `CallToolResult::error(vec![Content::text(...)])`
rather than bubbling `McpError`. This gives the LLM a readable explanation
instead of a protocol-level failure.

```rust
match self.client.get_listing_detail(&params.id).await {
    Ok(detail) => {
        let name = format!("Listing: {}", detail.name);
        Ok(self.finish(resource_uri::listing(&params.id), name, detail.to_string()).await)
    }
    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
        "Failed to get listing {}: {e}",
        quote_input(&params.id)
    ))])),
}
```

## Stdout discipline

**Stdout is reserved for MCP JSON-RPC.** Never `println!` in any code path
reachable by the MCP binary — that includes library code, adapter code,
and tests (use `eprintln!` in tests or `tracing::debug!`). Violating this
rule breaks the wire protocol silently and is painful to debug.

All logging goes through `tracing::{info,warn,error,debug}!` which is
configured in `main.rs` to write to `std::io::stderr()`.

## Testing tools

Integration tests in `tests/mcp_server_test.rs` define an inline
`IntegrationMock` struct implementing `AirbnbClient`. Follow that pattern
for new tool tests — do not depend on `test_helpers.rs` from integration
tests (it's `#[cfg(test)]`-gated at the library level, so it's invisible
to integration binaries).

For unit tests inside `src/mcp/server.rs`, use the `MockAirbnbClient`
from `test_helpers::` since they compile under `cfg(test)` for the library.
