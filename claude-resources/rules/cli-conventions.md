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
   - Data tool: validate the input (`validate_listing_id`, `SearchParams::validate`), call `client.<method>(...)`, then `output::render`.
   - Analytical tool: call the `application::analytical_handlers::run_*` function the MCP tool uses. Add it there first if it does not exist. Never import `domain::analytics` in `src/cli/`.

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

`render` returns a `String` (text mode passes through `output::sanitize_for_terminal`). Only `run()` writes, through `output::write_output`, which treats a closed pipe as success. `run()` returns an `ExitCode`: 0 success, 1 runtime failure, 2 invalid input. In `--json` mode, errors are printed on stdout as `{"error":{"kind","message"}}` (`render_error`). That covers clap's own argument errors too: `run()` calls `Cli::try_parse_from`, and when the command line asks for `--json`/`-j` (`json_requested`), `usage_error_report` turns the error into kind `invalid_params`, exit code 2. `--help` and `--version` are left to clap. Any range enforced by a clap value parser therefore still reaches `| jq` as JSON.

## Stdout vs stderr

Same discipline as the MCP server:

- **Stdout**: command output (the rendered result)
- **Stderr**: tracing logs, clap errors in text mode, panics

`init_tracing` in `src/cli/mod.rs` configures `tracing_subscriber::fmt`
to write to `std::io::stderr`. Never route it to stdout — that would
corrupt pipe usage like `airbnb --json search Paris | jq .`.

## Config discovery precedence

`application::load_app_config()` (both binaries):

1. `--config <path>` (CLI) or `AIRBNB_CONFIG`: explicit, must exist (error otherwise)
2. `$XDG_CONFIG_HOME/mcp-airbnb/config.yaml` (default `~/.config/mcp-airbnb/config.yaml`)
3. `config.yaml` next to the binary
4. Built-in defaults

The working directory is never searched: an MCP host sets it to whatever
project is open. Do not add a fourth lookup path.
