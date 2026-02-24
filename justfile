default: check

# Build and check
check:
    cargo check --all-targets

# Format code
fmt:
    cargo fmt --all

# Lint with clippy
lint:
    cargo clippy --all-targets -- -D warnings

# Run all tests
test:
    cargo test --all-targets

# Run coverage report
coverage:
    cargo tarpaulin --config tarpaulin.toml

# Security audit
audit:
    cargo deny check

# Run a specific fuzz target (requires nightly)
fuzz target="fuzz_search_parser" duration="60":
    cd fuzz && cargo +nightly fuzz run {{target}} -- -max_total_time={{duration}}

# Run all checks: format, lint, test, coverage, audit
all: fmt lint test coverage audit
