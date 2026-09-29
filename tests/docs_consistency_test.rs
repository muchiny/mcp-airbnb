//! Keeps the Markdown docs honest (DOC-1, DOC-2): tool, template and fuzz
//! target counts, tool names, documented commands, config keys, links, and
//! claims that were false during the 2026-09 audit are all checked against
//! the running server, the manifests and the config types.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mcp_airbnb::application::build_client;
use mcp_airbnb::config::types::Config;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const TOP_LEVEL_DOCS: [&str; 3] = ["README.md", "CLAUDE.md", "tests/README.md"];

/// Claims that were false when the 2026-09 audit ran, or that earlier phases
/// made false (P3 moved the limiter and removed `find_config_path`, P4
/// replaced the catch-all template); none may come back. Matched
/// case-insensitively. The bare key `respect_robots_txt` is allowed: P3's
/// "Removed keys" note in `src/config/README.md` has to name it.
const STALE_CLAIMS: [(&str, &str); 15] = [
    ("rmcp 0.16", "the crate uses rmcp 1.x"),
    (
        "serde_yml",
        "serde_yml was replaced by serde-saphyr in P0 (RUSTSEC-2025-0068)",
    ),
    (
        "honor airbnb",
        "respect_robots_txt never did anything; P3 removed it",
    ),
    (
        "airbnb://analysis/{type}",
        "P4 advertises one template per tool",
    ),
    ("optional methods", "every AirbnbClient method is required"),
    (
        "if parsing fails",
        "load_config returns YAML errors instead of defaults",
    ),
    (
        "HTTP 429 (no retry)",
        "429 handling lives in adapters::http::send_with_policy",
    ),
    (
        "extract_api_key_from_html",
        "the function is shared::extract_api_key",
    ),
    ("neighborhood_ttl_secs", "there is no such config key"),
    (
        "all business logic lives in the adapters",
        "orchestration lives in application/",
    ),
    (
        "parity is guaranteed",
        "CLI and MCP share code, not process state",
    ),
    ("token-bucket", "P3's limiter reserves slots under a lock"),
    ("token bucket", "P3's limiter reserves slots under a lock"),
    (
        "find_config_path",
        "P3 replaced it with application::load_app_config",
    ),
    (
        "scraper/rate_limiter",
        "P3 moved the limiter to src/adapters/rate_limiter.rs",
    ),
];

struct LiveSurface {
    tools: Vec<String>,
    templates: Vec<String>,
}

async fn live_surface() -> LiveSurface {
    let backend = build_client(Config::default())
        .expect("the default config builds a client without network I/O");
    let conn = common::connect(backend).await;
    let tools = conn
        .client
        .list_all_tools()
        .await
        .expect("tools/list should complete")
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let templates = conn
        .client
        .list_all_resource_templates()
        .await
        .expect("resources/templates/list should complete")
        .into_iter()
        .map(|template| template.raw.uri_template)
        .collect();
    conn.shutdown().await;
    LiveSurface { tools, templates }
}

fn read(rel: &str) -> String {
    read_path(&Path::new(ROOT).join(rel))
}

fn read_path(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn fuzz_target_count() -> usize {
    read("fuzz/Cargo.toml")
        .lines()
        .filter(|line| line.trim() == "[[bin]]")
        .count()
}

/// Every integer written right before `word` (e.g. `18 tools`), with its
/// line. A number that names a standard (`RFC 6570 templates`) is skipped.
fn numbers_before(text: &str, word: &str) -> Vec<(u64, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line
            .split(|c: char| {
                c.is_whitespace() || matches!(c, '(' | ')' | '*' | '`' | '|' | '[' | ']')
            })
            .filter(|token| !token.is_empty())
            .collect();
        for (index, pair) in tokens.windows(2).enumerate() {
            let next =
                pair[1].trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            let names_a_standard = index > 0 && tokens[index - 1].eq_ignore_ascii_case("rfc");
            if next == word
                && !names_a_standard
                && let Ok(number) = pair[0].parse::<u64>()
            {
                found.push((number, line.trim().to_string()));
            }
        }
    }
    found
}

#[test]
fn count_parser_skips_standard_numbers() {
    let text = "one RFC 6570 template per tool: 18 RFC 6570 templates, (18 templates)";
    let numbers: Vec<u64> = numbers_before(text, "templates")
        .into_iter()
        .map(|(number, _)| number)
        .collect();
    assert_eq!(numbers, vec![18]);
}

fn count_mismatches(text: &str, surface: &LiveSurface) -> Vec<String> {
    let normalized = text
        .replace("resource templates", "resource-templates")
        .replace("fuzz targets", "fuzz-targets");
    let expectations = [
        ("tools", surface.tools.len()),
        ("templates", surface.templates.len()),
        ("resource-templates", surface.templates.len()),
        ("fuzz-targets", fuzz_target_count()),
    ];
    let mut mismatches = Vec::new();
    for (word, expected) in expectations {
        for (number, line) in numbers_before(&normalized, word) {
            if usize::try_from(number).ok() != Some(expected) {
                mismatches.push(format!(
                    "says {number} {word} (expected {expected}): {line}"
                ));
            }
        }
    }
    mismatches
}

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

fn stale_claims(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut found: Vec<String> = STALE_CLAIMS
        .iter()
        .filter(|(phrase, _)| lower.contains(&phrase.to_lowercase()))
        .map(|(phrase, why)| format!("`{phrase}` ({why})"))
        .collect();
    if contains_word(text, "serde_yaml") {
        found.push("`serde_yaml` (the YAML crate was replaced in P0)".to_string());
    }
    found
}

/// Relative Markdown link targets outside fenced code blocks, without `#anchor`.
fn relative_links(markdown: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else {
                break;
            };
            let target = &after[..end];
            let external = target.contains("://") || target.starts_with("mailto:");
            if !target.is_empty() && !target.starts_with('#') && !external {
                links.push(target.split('#').next().unwrap_or(target).to_string());
            }
            rest = &after[end..];
        }
    }
    links
}

/// (top-level `scraper` + `cache` keys, `scraper.graphql_hashes` keys)
fn config_keys() -> (BTreeSet<String>, BTreeSet<String>) {
    let value = serde_json::to_value(Config::default()).expect("Config serializes");
    let mut keys = BTreeSet::new();
    for section in ["scraper", "cache"] {
        let object = value
            .get(section)
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("Config has no `{section}` object"));
        keys.extend(object.keys().cloned());
    }
    let hash_keys = value
        .pointer("/scraper/graphql_hashes")
        .and_then(serde_json::Value::as_object)
        .map(|hashes| hashes.keys().cloned().collect())
        .unwrap_or_default();
    (keys, hash_keys)
}

/// A key is documented as `` `key` `` or as the tail of `` `section.key` ``.
fn documents_key(doc: &str, key: &str) -> bool {
    doc.contains(&format!("`{key}`")) || doc.contains(&format!(".{key}`"))
}

fn words_after<'a>(line: &'a str, prefix: &str) -> Vec<&'a str> {
    line.match_indices(prefix)
        .filter_map(|(index, _)| line[index + prefix.len()..].split_whitespace().next())
        .filter(|word| !word.starts_with('<') && !word.starts_with('-'))
        .collect()
}

#[tokio::test]
async fn top_level_docs_state_the_real_counts() {
    let surface = live_surface().await;
    let mut problems = Vec::new();
    for doc in TOP_LEVEL_DOCS {
        problems.extend(
            count_mismatches(&read(doc), &surface)
                .into_iter()
                .map(|problem| format!("{doc}: {problem}")),
        );
    }
    for doc in ["README.md", "CLAUDE.md"] {
        let text = read(doc).replace("resource templates", "resource-templates");
        let tools = format!("{} tools", surface.tools.len());
        let templates = format!("{} resource-templates", surface.templates.len());
        if !text.contains(&tools) {
            problems.push(format!("{doc}: never says `{tools}`"));
        }
        if !text.contains(&templates) {
            problems.push(format!(
                "{doc}: never says `{} resource templates`",
                surface.templates.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[tokio::test]
async fn every_registered_tool_and_template_is_documented() {
    let surface = live_surface().await;
    let mut missing = Vec::new();
    for doc in ["README.md", "CLAUDE.md"] {
        let text = read(doc);
        for tool in &surface.tools {
            if !text.contains(&format!("`{tool}`")) {
                missing.push(format!("{doc}: tool `{tool}`"));
            }
        }
        for template in &surface.templates {
            if !text.contains(&format!("`{template}`")) {
                missing.push(format!("{doc}: template `{template}`"));
            }
        }
    }
    assert!(missing.is_empty(), "undocumented:\n{}", missing.join("\n"));
}

#[test]
fn documented_cargo_commands_are_runnable() {
    assert!(
        read("Cargo.toml").contains("default-run = \"mcp-airbnb\""),
        "Cargo.toml needs default-run so a bare `cargo run` starts the server (DOC-1)"
    );
    let fuzz_manifest = read("fuzz/Cargo.toml");
    let mut problems = Vec::new();
    for doc in TOP_LEVEL_DOCS {
        for line in read(doc).lines() {
            if line.contains("cargo run") && !line.contains("--bin ") {
                problems.push(format!("{doc}: `cargo run` without --bin: {line}"));
            }
            if line.contains("\"run\"") && !line.contains("\"--bin\"") {
                problems.push(format!("{doc}: cargo args without \"--bin\": {line}"));
            }
            for name in words_after(line, "cargo test --test ") {
                if !Path::new(ROOT)
                    .join("tests")
                    .join(format!("{name}.rs"))
                    .is_file()
                {
                    problems.push(format!("{doc}: no test target named {name}: {line}"));
                }
            }
            for name in words_after(line, "fuzz run ") {
                if !fuzz_manifest.contains(&format!("name = \"{name}\"")) {
                    problems.push(format!("{doc}: no fuzz target named {name}: {line}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn config_keys_are_documented() {
    let (keys, _) = config_keys();
    assert!(
        keys.contains("currency") && keys.contains("locale"),
        "P1b (I4) adds scraper.currency and scraper.locale"
    );
    let mut missing = Vec::new();
    for doc in ["README.md", "CLAUDE.md"] {
        let text = read(doc);
        for key in &keys {
            if !documents_key(&text, key) {
                missing.push(format!("{doc}: `{key}`"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "undocumented config keys:\n{}",
        missing.join("\n")
    );
}

#[test]
fn top_level_docs_repeat_no_known_false_claims() {
    let mut problems = Vec::new();
    for doc in TOP_LEVEL_DOCS {
        for claim in stale_claims(&read(doc)) {
            problems.push(format!("{doc}: {claim}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn readme_relative_links_resolve() {
    let broken: Vec<String> = relative_links(&read("README.md"))
        .into_iter()
        .filter(|link| !Path::new(ROOT).join(link).exists())
        .collect();
    assert!(
        broken.is_empty(),
        "README.md links to missing files: {broken:?}"
    );
}

#[test]
fn readme_documents_known_upstream_limitations() {
    let readme = read("README.md");
    let section = readme
        .split("## ⚠️ Known upstream limitations")
        .nth(1)
        .expect("README needs a `## ⚠️ Known upstream limitations` section (DOC-2)")
        .split("\n## ")
        .next()
        .unwrap_or_default()
        .to_lowercase();
    for topic in ["calendar", "persisted", "currency", "rate limit"] {
        assert!(
            section.contains(topic),
            "the limitations section must cover `{topic}`"
        );
    }
}

fn rel_path(path: &Path) -> String {
    path.strip_prefix(ROOT)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Every file with extension `ext` below `rel_dir`, sorted.
fn files_under(rel_dir: &str, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                walk(&path, ext, out);
            } else if path.extension().is_some_and(|found| found == ext) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(ROOT).join(rel_dir), ext, &mut out);
    out.sort();
    out
}

fn markdown_under(rel_dir: &str) -> Vec<PathBuf> {
    files_under(rel_dir, "md")
}

/// Every `.rs` file below `rel_dir`, sorted.
fn rust_files_under(rel_dir: &str) -> Vec<PathBuf> {
    files_under(rel_dir, "rs")
}

fn component_docs() -> Vec<PathBuf> {
    let mut docs = markdown_under("src");
    docs.extend(markdown_under("claude-resources"));
    docs
}

/// The body of the first fenced block opened with three backticks + `lang`,
/// or "" when there is none.
fn fenced_block<'a>(markdown: &'a str, lang: &str) -> &'a str {
    let open = format!("```{lang}\n");
    markdown
        .split_once(open.as_str())
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map_or("", |(block, _)| block)
}

#[tokio::test]
async fn component_docs_state_the_real_counts_and_names() {
    let surface = live_surface().await;
    let mut problems = Vec::new();
    for path in component_docs() {
        let text = read_path(&path);
        problems.extend(
            count_mismatches(&text, &surface)
                .into_iter()
                .map(|problem| format!("{}: {problem}", rel_path(&path))),
        );
    }
    let mcp_readme = read("src/mcp/README.md");
    for tool in &surface.tools {
        if !mcp_readme.contains(&format!("`{tool}`")) {
            problems.push(format!("src/mcp/README.md: tool `{tool}` missing"));
        }
    }
    for template in &surface.templates {
        if !mcp_readme.contains(&format!("`{template}`")) {
            problems.push(format!("src/mcp/README.md: template `{template}` missing"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn component_docs_repeat_no_known_false_claims() {
    let mut problems = Vec::new();
    for path in component_docs() {
        for claim in stale_claims(&read_path(&path)) {
            problems.push(format!("{}: {claim}", rel_path(&path)));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn component_readme_links_resolve() {
    let mut broken = Vec::new();
    for path in markdown_under("src") {
        let base = path.parent().unwrap_or(Path::new(ROOT));
        for link in relative_links(&read_path(&path)) {
            if !base.join(&link).exists() {
                broken.push(format!("{}: {link}", rel_path(&path)));
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

#[test]
fn config_readme_documents_every_key_and_the_validation_rules() {
    let (keys, hash_keys) = config_keys();
    let text = read("src/config/README.md");
    let example = fenced_block(&text, "yaml");
    let diagram = fenced_block(&text, "mermaid");
    let mut missing = Vec::new();
    for key in keys.iter().chain(&hash_keys) {
        if !documents_key(&text, key) {
            missing.push(format!("table: {key}"));
        }
        if !example
            .lines()
            .any(|line| line.trim_start().starts_with(&format!("{key}:")))
        {
            missing.push(format!("yaml example: {key}"));
        }
        if !diagram
            .lines()
            .any(|line| line.trim_end().ends_with(&format!(" {key}")))
        {
            missing.push(format!("class diagram: {key}"));
        }
    }
    assert!(
        missing.is_empty(),
        "src/config/README.md misses {missing:?}"
    );
    assert!(
        text.contains("Config::validate"),
        "src/config/README.md must describe Config::validate (I5)"
    );
}

#[test]
fn adapters_readme_lists_every_adapter_file() {
    let readme = read("src/adapters/README.md");
    let structure = readme
        .split("## 📂 Structure")
        .nth(1)
        .expect("src/adapters/README.md needs a `## 📂 Structure` section")
        .split("\n## ")
        .next()
        .unwrap_or_default();
    assert!(!structure.trim().is_empty(), "`## 📂 Structure` is empty");
    let missing: Vec<String> = rust_files_under("src/adapters")
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| name != "mod.rs" && !structure.contains(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "src/adapters/README.md `## 📂 Structure` does not list {missing:?}"
    );
}

#[test]
fn scraping_conventions_point_at_the_anonymized_fixture_layout() {
    let text = read("claude-resources/rules/scraping-conventions.md");
    assert!(text.contains("tests/fixtures/airbnb/"), "fixture path (I6)");
    assert!(
        text.to_lowercase().contains("anonymiz"),
        "privacy rule for fixtures"
    );
    assert!(
        text.contains("UpstreamSchema"),
        "drift is an error, not Ok(empty) (I2)"
    );
}

/// The `/fixtures` skill is an instruction file agents act on (DOC-2): its
/// capture walkthrough must keep raw captures, and the personal data in
/// them, out of git.
#[test]
fn fixtures_skill_keeps_raw_captures_out_of_git() {
    const RULE: &str = "Raw captures (real names, reviews) never enter git";
    let skill = read("claude-resources/skills/fixtures/SKILL.md");
    assert!(
        read("CLAUDE.md").contains(RULE),
        "CLAUDE.md no longer states the rule"
    );
    assert!(
        skill.contains(RULE),
        "the skill must quote CLAUDE.md's rule"
    );
    let description = skill
        .lines()
        .find(|line| line.starts_with("description:"))
        .unwrap_or("");
    assert!(
        description.contains("tests/fixtures/airbnb/"),
        "frontmatter description must name the fixture directory: {description}"
    );
    let lower = skill.to_lowercase();
    assert!(
        !lower.contains("gzip"),
        "no option may commit the full HTML page"
    );
    assert!(
        !lower.contains("fixtures are public data"),
        "fixtures hold third-party personal data until anonymized"
    );
    let html_fixtures: Vec<&str> = skill
        .lines()
        .filter(|line| line.contains("tests/fixtures/airbnb/") && line.contains(".html"))
        .collect();
    assert!(
        html_fixtures.is_empty(),
        "committed fixtures are JSON islands, never HTML pages: {html_fixtures:?}"
    );
    for needle in [
        "data-deferred-state",
        "scripts/anonymize_fixtures.py",
        "COMMITTED_FIXTURES",
        "fixtures_are_anonymized",
        "a0.muscache.com",
    ] {
        assert!(
            skill.contains(needle),
            "the privacy step must mention {needle}"
        );
    }
}
