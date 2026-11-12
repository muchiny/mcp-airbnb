# Claude Code Resources for `mcp-airbnb`

Project-specific Claude Code resources (rules, skills, agents) bundled with
this repository. This folder is **committed to git** — unlike `.claude/`
which is gitignored — so anyone cloning the repo can link or copy them into
their own Claude Code setup (see Installation).

## What's in here

```
claude-resources/
├── rules/
│   ├── mcp-conventions.md         # How to add/modify an MCP tool (rmcp 3.x macros, ResourceStore, tests)
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

Claude Code only loads what sits in `.claude/` (project) or `~/.claude/`
(user). Nothing reads `claude-resources/` by itself: link or copy it first.
The two modes can coexist.

### Mode A — Project-level, linked (recommended inside this repo)

`.claude/` is gitignored here, so the links stay local. Link each
sub-folder, not `.claude/` itself: Claude Code writes `settings.local.json`
and its worktrees into `.claude/`, and those must not land in the tracked
`claude-resources/` folder.

```bash
# From the repo root
mkdir -p .claude
ln -s ../claude-resources/rules  .claude/rules
ln -s ../claude-resources/skills .claude/skills
ln -s ../claude-resources/agents .claude/agents
```

Check that nothing is shadowed first. A project agent or skill with the
same `name` as a user-level one of `~/.claude/` is resolved by priority
instead of coexisting (`grep -h '^name:' ~/.claude/agents/*.md`). The names
used here are `mcp-tool-builder`, `scraper-debugger` (agents) and
`mcp-smoke`, `cli-demo`, `fixtures` (skills).

Start a new Claude Code session from the repo root. The three rules use
`paths:` frontmatter, so each one loads only once Claude reads or edits a
matching file (for example `src/cli/**` for `cli-conventions.md`). Skills
and agents are available straight away.

### Mode B — User-level copy (available from any directory)

```bash
# From the repo root
cp -r claude-resources/rules/*  ~/.claude/rules/
cp -r claude-resources/skills/* ~/.claude/skills/
cp -r claude-resources/agents/* ~/.claude/agents/
```

Files with the same name in `~/.claude/` are overwritten: review before
copying. A copy does not follow later edits of `claude-resources/`, so
repeat the copy after each update.

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

`/cli-demo` has `disable-model-invocation: true` because its search step
hits the real Airbnb site: it only runs when you type it. The other skills
and the agents stay available to Claude.

When installed user-level, the skills and agents only make sense inside this
repo (they read `src/` and `tests/` by relative path); `/mcp-smoke` prints
`not in an mcp-airbnb project` and stops elsewhere.

## Updating

When the project conventions change, update the files in
`claude-resources/` and commit them. `tests/docs_consistency_test.rs` also
reads these files (tool, template and fuzz-target counts, the fixture
privacy rule), so a stale count here fails the test suite. User-level Rust
conventions (`~/.claude/rules/`) are orthogonal and stay where they are —
this folder only ships what's **specific to mcp-airbnb**.
