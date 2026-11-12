---
paths:
  - "src/mcp/**/*.rs"
  - "src/application/analytical_handlers.rs"
  - "tests/mcp_server_test.rs"
  - "tests/mcp_resources_test.rs"
  - "tests/functional_verification_test.rs"
---

# MCP Server Conventions (`mcp-airbnb`)

Rules specific to the MCP protocol layer in `src/mcp/`. Applies when adding,
modifying, or debugging MCP tools.

## rmcp 3.x macros

Tools are declared via three attribute macros from `rmcp`:

```rust
#[tool_router]                  // on the `impl AirbnbMcpServer` block
#[tool(name = "...", description = "...", annotations(...))]
                                // on each async tool method
#[tool_handler]                 // on the `impl ServerHandler for AirbnbMcpServer`
```

See `src/mcp/server.rs` for 21 existing examples. Do not roll your own
JSON-RPC dispatch — always go through the macros.

## Tool handler shape

A tool handler is a **thin wrapper**. It must:

1. Take `Parameters(params): Parameters<MyToolParams>`.
2. Check every input with `check_input!(...)`: `validate_listing_id`, `limits::months`, `limits::count_in_range`, `SearchParams::validate`, … A failed check returns `isError` with the limit in the text. Nothing is clamped and nothing is fetched.
3. Data tools call `self.client.<method>(...)`. Analytical tools call the `application::analytical_handlers::run_*` function that the CLI also calls. Never orchestrate fetches or call `domain::analytics::compute_*` in `src/mcp/` (`tests/parity_test.rs` checks this).
4. Render with the result's `Display` and end with `Ok(self.finish(resource_uri::<kind>(…), name, text).await)`. `finish` fences the text as untrusted, stores it, and sends `resources/list_changed` when the URI is new.

## Parameter structs

- Live in `src/mcp/server.rs`, above the `impl AirbnbMcpServer` block, one
  `<Name>ToolParams` struct per tool
- Must derive `Debug, serde::Deserialize, schemars::JsonSchema`
  (`rmcp` is built with the `schemars` feature, see `Cargo.toml`)
- Doc-comments on each field become the JSON schema `description` — write
  them as if the LLM will read them (it will)
- Optional fields: `Option<T>` with no default
- Ranges: declare them with `#[schemars(range(min = limits::X, max = limits::Y))]` / `#[schemars(length(...))]` / `#[schemars(pattern(LISTING_ID_PATTERN))]`, using the constants in `src/domain/limits.rs`, and enforce the same constants at runtime. Never `.clamp()`. Every parameter struct carries `#[serde(deny_unknown_fields)]`.

## Resource storage

Every successful tool result is stored through `finish`. URIs come only from the builders in `src/mcp/resource_uri.rs` (one RFC 6570 template per tool, 21 in total; every value percent-encoded; every result-changing input in the URI). `ResourceStore` is a bounded LRU: 256 entries, 8 MiB, 1 h TTL, URIs ≤ 16 KiB. `resources/list` returns pages of 100. Add a template to `TEMPLATES` whenever you add a tool.

## Error handling

Return an error **inside** `CallToolResult::error(vec![ContentBlock::text(...)])`
rather than bubbling `McpError`. This gives the LLM a readable explanation
instead of a protocol-level failure.

```rust
// Abridged from `airbnb_listing_details` in src/mcp/server.rs
check_input!(validate_listing_id(&params.id));
match self.client.get_listing_detail(&params.id).await {
    Ok(detail) => {
        let text = detail.to_string();
        let name = format!("Listing: {}", detail.name);
        Ok(self.finish(resource_uri::listing(&params.id), name, text).await)
    }
    Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
        "Failed to get listing details for ID {}: {e}. Verify the listing ID is correct — use airbnb_search to find valid IDs.",
        quote_input(&params.id)
    ))])),
}
```

`quote_input` (`src/error.rs`) escapes and truncates user input before it is
echoed back, so an id cannot inject text into the message.

## Stdout discipline

**Stdout is reserved for MCP JSON-RPC.** Never `println!` in any code path
reachable by the MCP binary — that includes library code, adapter code,
and tests (use `eprintln!` in tests or `tracing::debug!`). Violating this
rule breaks the wire protocol silently and is painful to debug.

All logging goes through `tracing::{info,warn,error,debug}!` which is
configured in `main.rs` to write to `std::io::stderr()`.

## Adding a tool also touches

`resource_uri::TEMPLATES` (and its length), the tool tables and counts in
`README.md`, `CLAUDE.md` and `src/mcp/README.md`, and the hard-coded counts and
tool list in `tests/mcp_server_test.rs` (`EXPECTED_TOOLS`) and
`tests/mcp_resources_test.rs`. `tests/docs_consistency_test.rs` fails when a
count or a tool/template name in the docs drifts from the live server.

## Testing tools

Integration tests in `tests/mcp_server_test.rs` define an inline
`IntegrationMock` struct implementing `AirbnbClient`. Follow that pattern
for new tool tests — do not depend on `test_helpers.rs` from integration
tests (it's `#[cfg(test)]`-gated at the library level, so it's invisible
to integration binaries).

For unit tests inside `src/mcp/server.rs`, use the `MockAirbnbClient`
from `test_helpers::` since they compile under `cfg(test)` for the library.
