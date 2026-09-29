# Claude Code Resources for `mcp-airbnb`

Project-specific Claude Code resources (rules, skills, agents) bundled with
this repository. This folder is **committed to git** — unlike `.claude/`
which is gitignored — so anyone cloning the repo can install them into their
own Claude Code setup.

## What's in here

```
claude-resources/
├── rules/
│   ├── mcp-conventions.md         # How to add/modify an MCP tool (rmcp 1.x macros, ResourceStore, tests)
│   ├── scraping-conventions.md    # Rate limiting, fixtures, stdout/stderr, composite client pattern
│   └── cli-conventions.md         # How to add a new subcommand to the `airbnb` CLI
├── skills/
│   ├── mcp-smoke/SKILL.md         # /mcp-smoke — smoke-test the MCP server via stdin
│   ├── cli-demo/SKILL.md          # /cli-demo — run the `airbnb` CLI with sample queries
│   └── fixtures/SKILL.md          # /fixtures — list, capture and anonymize JSON test fixtures
└── agents/
    ├── mcp-tool-builder.md        # Scaffolds a new MCP tool following project conventions
    └── scraper-debugger.md        # Diagnoses HTML/GraphQL parsing failures
```

## Installation

Two installation modes — pick the one that fits your workflow. Both work
simultaneously (user-level + project-level) without conflict.

### Mode A — User-level (recommended for daily use)

Copy the resources into your own `~/.claude/` so they're available from any
working directory, not just when you're inside `mcp-airbnb/`.

```bash
# From the repo root
cp -r claude-resources/rules/*  ~/.claude/rules/
cp -r claude-resources/skills/* ~/.claude/skills/
cp -r claude-resources/agents/* ~/.claude/agents/
```

If a file with the same name already exists in `~/.claude/`, it will be
overwritten — review before copying or rename the project files first.

### Mode B — Project-level (automatic discovery)

Claude Code auto-discovers `.claude/rules/`, `.claude/skills/`, and
`.claude/agents/` in the current working directory. `.claude/` is gitignored
in this repo, so you can symlink it to `claude-resources/` locally without
polluting the git history:

```bash
# From the repo root
ln -s claude-resources .claude
```

This gives the resources to anyone running Claude Code from inside
`mcp-airbnb/`. Symlinks are fine — Claude Code walks through them.

If you prefer a copy (e.g. you want to tweak files locally without touching
the committed versions), use `cp -r claude-resources/* .claude/` instead.

## What requires what

Most resources assume a working Rust toolchain and that you're inside the
`mcp-airbnb` project. Specifically:

| Resource | Requires |
|----------|----------|
| `rules/*.md` | Nothing — they're pure guidance, always safe to install |
| `skills/mcp-smoke` | `cargo`, built `mcp-airbnb` binary |
| `skills/cli-demo` | `cargo`, built `airbnb` binary |
| `skills/fixtures` | `tests/fixtures/airbnb/` folder (part of the repo), `scripts/anonymize_fixtures.py` |
| `agents/mcp-tool-builder` | Read access to `src/mcp/`, `src/ports/`, `src/domain/` |
| `agents/scraper-debugger` | Read access to `src/adapters/scraper/`, `tests/fixtures/` |

All skills/agents are safe when installed user-level — they'll simply fail
gracefully (with a "not a mcp-airbnb project" hint) when invoked outside
this repo.

## Updating

When the project conventions change, update the files in
`claude-resources/` and commit them. The workspace-level rules in
`~/dev/.claude/rules/` (hexagonal architecture, Rust conventions, WSL
safety) are orthogonal and stay where they are — this folder only ships
what's **specific to mcp-airbnb**.
