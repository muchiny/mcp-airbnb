# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.2.x   | Yes       |
| < 0.2   | No        |

## Reporting a Vulnerability

If you discover a security vulnerability in mcp-airbnb, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Use [GitHub Security Advisories](../../security/advisories/new) to report privately.

### What to Include

- Description of the vulnerability
- Steps to reproduce (or a minimal input that triggers it)
- Potential impact
- Suggested fix (if any)

## Scope

mcp-airbnb fetches and parses public web data and exposes it through an MCP server and a CLI. Reports about parser crashes, unbounded resource use on hostile responses, request-forgery or injection through tool inputs, and leakage of local configuration are in scope.

## Supply Chain

- `unsafe_code` is forbidden in this crate (`unsafe_code = "forbid"`).
- Dependencies are checked in CI with `cargo deny` (advisories, licenses, bans, sources; yanked crates are denied).
- `Cargo.lock` is committed and CI builds with `--locked`.
