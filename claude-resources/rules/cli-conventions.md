# CLI Conventions (`airbnb` binary)

Rules specific to the `airbnb` CLI binary and the `src/cli/` module.
The CLI is a second entry point alongside the MCP server — both binaries
share the `src/application/build_client()` wiring, so most conventions
are about keeping that boundary clean.

## Two binaries, one library

- `src/main.rs` → `mcp-airbnb` (MCP server over stdio)
- `src/bin/cli.rs` → `airbnb` (CLI with clap subcommands)

Both are declared as explicit `[[bin]]` entries in `Cargo.toml`. Shared
logic **must** live in the library under `src/application/` — never
duplicate code between the two `main` functions.

## Adding a new CLI subcommand

1. **Define the args struct** in `src/cli/args.rs`. Reuse existing shapes
   where possible:
   - Listing ID → `IdArgs`
   - Location → `LocationArgs`
   - ID + months → `CalendarArgs`
   - ID + optional location → `IdLocationArgs`
   - ID + optional location + months → `IdMonthsLocationArgs`
   - Custom shape: add a new `#[derive(Debug, clap::Args)]` struct

2. **Add a variant** to the `Commands` enum in `src/cli/args.rs`, then
   add a unit test asserting it parses correctly.

3. **Add a match arm** in `src/cli/mod.rs::dispatch`. Choose the right
   pattern:
   - Pure data tool → call `client.<method>(...)` directly and `output::render`
   - Single-fetch analytical → call `client.<method>(...)` then a
     `domain::analytics::compute_*` function inline
   - Multi-fetch analytical → add a helper to
     `src/application/analytical_handlers.rs` and call it from `dispatch`

4. **Add an integration test** in `tests/cli_test.rs` using the inline
   `CliMock` struct. Assert on the rendered string (text mode) or parse
   the JSON output with `serde_json::from_str`.

5. **Do not add business logic to the CLI layer.** Computations live in
   `domain::analytics`, fetches live behind the `AirbnbClient` trait,
   orchestration lives in `application::analytical_handlers`. The CLI is
   a thin translator: args in, render out.

## Global flags

All three global flags on `Cli` are already wired:

- `--config/-c <PATH>` (also reads `AIRBNB_CONFIG` env var)
- `--json/-j` — machine-readable output via `serde_json::to_string_pretty`
- `-v`/`-vv` — tracing verbosity (default: warn, -v: info, -vv: debug)

Do not add new global flags without considering whether they should also
apply to the MCP server (most don't).

## Output rendering

The `output::render<T: Serialize + Display>(value, as_json) -> String`
helper is the single point of rendering. It uses:

- `Display` for human-readable text mode
- `serde_json::to_string_pretty` for JSON mode

Every result type exposed by a subcommand **must** implement both
`Serialize` and `Display`. All domain types already do — if you introduce
a new one in `src/domain/`, add a `Display` impl next to the
`Serialize` derive and cover it with a test in the same file.

`render` returns a `String` rather than printing to stdout so `dispatch`
stays testable. Only `run()` (in `src/cli/mod.rs`) should ever call
`print!`/`println!`.

## Stdout vs stderr

Same discipline as the MCP server:

- **Stdout**: command output (the rendered result)
- **Stderr**: tracing logs, clap errors, panics

`init_tracing` in `src/cli/mod.rs` configures `tracing_subscriber::fmt`
to write to `std::io::stderr`. Never route it to stdout — that would
corrupt pipe usage like `airbnb --json search Paris | jq .`.

## Config discovery precedence

`application::find_config_path()`:

1. `AIRBNB_CONFIG` env var (if set and file exists)
2. `./config.yaml` in CWD
3. `config.yaml` next to the binary
4. Falls back to `./config.yaml` (non-existent → loader returns defaults)

The `--config` flag on the CLI overrides all of the above. Do not add
a fourth lookup path — three is already one more than most tools ship.
