# MCP Server Conventions (`mcp-airbnb`)

Rules specific to the MCP protocol layer in `src/mcp/`. Applies when adding,
modifying, or debugging MCP tools.

## rmcp 0.16 macros

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

A tool handler is a **thin wrapper** around an `AirbnbClient` trait call
(or, for analytical tools, a `domain::analytics::compute_*` function). It
must:

1. Take `Parameters(params): Parameters<MyToolParams>` where `MyToolParams`
   is `#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]`
2. Call `self.client.<method>(...)` (or a `compute_*` function)
3. Format the result as text via the domain type's `Display` impl
4. Store the result as an MCP resource via `self.resources.insert(uri, name, text.clone())`
5. Return `CallToolResult::success(vec![Content::text(text)])`

**Never** put business logic in a tool handler — it belongs in
`src/domain/analytics.rs` as a pure `compute_*` function. Handlers orchestrate
fetches + rendering, nothing else.

## Parameter structs

- Live in `src/mcp/server.rs` near the top (or in the handler method if
  single-use)
- Must derive `Debug, serde::Deserialize, schemars::JsonSchema`
- Doc-comments on each field become the JSON schema `description` — write
  them as if the LLM will read them (it will)
- Optional fields: `Option<T>` with no default
- Numeric ranges: validate manually in the handler with `.clamp(min, max)`
  — clap-style range validators don't exist in schemars

## Resource storage

Every successful tool result is stored as an MCP resource so clients can
reference previously-fetched data without re-scraping. URI conventions:

| Data type         | URI pattern                                |
|-------------------|--------------------------------------------|
| Listing           | `airbnb://listing/{id}`                    |
| Calendar          | `airbnb://listing/{id}/calendar`           |
| Reviews           | `airbnb://listing/{id}/reviews`            |
| Host profile      | `airbnb://listing/{id}/host`               |
| Search result     | `airbnb://search/{location}`               |
| Neighborhood      | `airbnb://neighborhood/{location}`         |
| Analytics         | `airbnb://analysis/{kind}/{id_or_key}`     |

The `ResourceStore` is a `RwLock<HashMap<String, ResourceEntry>>` — cheap
inserts, no eviction. Don't worry about unbounded growth in v1; it's per-
session and the MCP client decides when to `ReadResource`.

## Error handling

Return an error **inside** `CallToolResult::error(vec![Content::text(...)])`
rather than bubbling `McpError`. This gives the LLM a readable explanation
instead of a protocol-level failure.

```rust
match self.client.get_listing_detail(&id).await {
    Ok(detail) => Ok(CallToolResult::success(vec![Content::text(detail.to_string())])),
    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
        "Failed to get listing '{id}': {e}"
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
