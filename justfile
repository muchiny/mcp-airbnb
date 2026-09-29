# Pass recipe arguments as positional shell args so `smoke` forwards them intact
set positional-arguments

default: check

# Build and check
check:
    cargo check --all-targets

# Format code
fmt:
    cargo fmt --all

# Lint with clippy (MSRV toolchain from rust-toolchain.toml)
lint:
    cargo clippy --all-targets -- -D warnings

# Preview lints from the latest stable clippy (advisory; mirrors the CI clippy-latest job)
lint-latest:
    cargo +stable clippy --all-targets -- -D warnings

# Build the API docs with warnings denied (mirrors the CI doc job)
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Run all tests
test:
    cargo test --all-targets

# Run coverage report
coverage:
    cargo tarpaulin --config tarpaulin.toml

# Security audit (RustSec advisories, licenses, sources)
audit:
    cargo deny check
    cargo audit

# Check GitHub workflow hygiene (SHA-pinned actions, --locked, toolchain pin)
workflows:
    python3 scripts/check_workflows.py

# Run one fuzz target with the committed seeds and dictionary (nightly + cargo-fuzz)
fuzz target="fuzz_search_parser" duration="60":
    mkdir -p fuzz/corpus/{{target}}
    cargo +nightly fuzz run {{target}} {{justfile_directory()}}/fuzz/corpus/{{target}} {{justfile_directory()}}/fuzz/seeds/{{target}} -- -dict={{justfile_directory()}}/fuzz/airbnb.dict -max_total_time={{duration}} -max_len=131072 -timeout=10

# Regenerate fuzz/seeds/ from tests/fixtures/airbnb/2026-09/ (anonymized fixtures only)
fuzz-seeds:
    python3 fuzz/make_seeds.py

# Install git pre-commit hooks via lefthook (requires lefthook binary on PATH)
install-hooks:
    lefthook install

# Run all checks: format, lint, docs, test, coverage, audit, workflow hygiene
all: fmt lint doc test coverage audit workflows

# Offline tests of the live smoke harness (no network)
smoke-selftest:
    python3 -m unittest discover -s scripts -p 'test_live_smoke.py' -v

# Opt-in live smoke test against real Airbnb (rate-limited; never in CI)
smoke *args:
    cargo build --release --bin mcp-airbnb
    python3 scripts/live_smoke.py --live "$@"
