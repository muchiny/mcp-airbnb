//! Replays every committed fuzz seed (`fuzz/seeds/<target>/*`) through the
//! `mcp_airbnb::fuzz_support` harness on the stable toolchain, proves the
//! seeds reach past each parser's structural gate, and checks that every
//! fuzz target is wired in `fuzz/Cargo.toml` and `fuzz/fuzz_targets/`.

use std::collections::BTreeSet;
use std::path::Path;

use mcp_airbnb::fuzz_support;

type Harness = fn(&[u8]) -> bool;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// (cargo-fuzz target, harness function name, harness, must at least one
/// seed be accepted by the parser?)
const TARGETS: [(&str, &str, Harness, bool); 13] = [
    (
        "fuzz_search_parser",
        "scraper_search_html",
        fuzz_support::scraper_search_html,
        true,
    ),
    (
        "fuzz_detail_parser",
        "scraper_detail_html",
        fuzz_support::scraper_detail_html,
        true,
    ),
    (
        "fuzz_calendar_parser",
        "scraper_calendar",
        fuzz_support::scraper_calendar,
        true,
    ),
    (
        "fuzz_review_parser",
        "scraper_reviews_html",
        fuzz_support::scraper_reviews_html,
        true,
    ),
    (
        "fuzz_graphql_search",
        "graphql_search",
        fuzz_support::graphql_search,
        true,
    ),
    (
        "fuzz_graphql_detail",
        "graphql_detail",
        fuzz_support::graphql_detail,
        true,
    ),
    (
        "fuzz_graphql_review",
        "graphql_reviews",
        fuzz_support::graphql_reviews,
        true,
    ),
    (
        "fuzz_graphql_host",
        "graphql_host",
        fuzz_support::graphql_host,
        true,
    ),
    (
        "fuzz_host_profile_html",
        "scraper_host_profile_html",
        fuzz_support::scraper_host_profile_html,
        true,
    ),
    ("fuzz_api_key", "api_key", fuzz_support::api_key, true),
    (
        "fuzz_calendar_analytics",
        "calendar_to_analytics",
        fuzz_support::calendar_to_analytics,
        true,
    ),
    (
        "fuzz_calendar_model",
        "calendar_model_analytics",
        fuzz_support::calendar_model_analytics,
        true,
    ),
    (
        "fuzz_input_validation",
        "input_validation",
        fuzz_support::input_validation,
        true,
    ),
];

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(ROOT).join(rel))
        .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

fn load_seeds(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(ROOT).join("fuzz/seeds").join(target);
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "missing seed directory {}: {e} (run python3 fuzz/make_seeds.py)",
            dir.display()
        )
    });
    let mut seeds: Vec<(String, Vec<u8>)> = entries
        .map(|entry| {
            let path = entry
                .unwrap_or_else(|e| panic!("unreadable entry in {}: {e}", dir.display()))
                .path();
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, bytes)
        })
        .collect();
    seeds.sort();
    seeds
}

#[test]
fn every_seed_replays_without_panicking() {
    for (target, _, harness, _) in TARGETS {
        let seeds = load_seeds(target);
        assert!(!seeds.is_empty(), "{target} has no committed seed");
        for (name, bytes) in &seeds {
            let outcome = std::panic::catch_unwind(|| harness(bytes));
            assert!(
                outcome.is_ok(),
                "{target}: seed {name} panicked (this aborts the release server)"
            );
        }
    }
}

#[test]
fn seeds_reach_past_the_structural_gate() {
    for (target, _, harness, must_reach) in TARGETS {
        if !must_reach {
            continue;
        }
        let accepted = load_seeds(target).iter().any(|(_, bytes)| harness(bytes));
        assert!(
            accepted,
            "no seed of {target} is accepted by its parser, so the fuzzer would only \
             exercise error paths; regenerate with python3 fuzz/make_seeds.py or fix \
             the page wrapper there"
        );
    }
}

#[test]
fn fuzz_targets_are_wired_in_the_fuzz_crate() {
    let manifest = read("fuzz/Cargo.toml");
    let bins: BTreeSet<String> = manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| name.starts_with("fuzz_"))
        .map(str::to_string)
        .collect();
    let expected: BTreeSet<String> = TARGETS
        .iter()
        .map(|(target, ..)| (*target).to_string())
        .collect();
    assert_eq!(bins, expected, "fuzz/Cargo.toml [[bin]] names");
    let sources: BTreeSet<String> = std::fs::read_dir(Path::new(ROOT).join("fuzz/fuzz_targets"))
        .expect("fuzz/fuzz_targets exists")
        .filter_map(|entry| {
            entry
                .ok()?
                .path()
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    assert_eq!(sources, expected, "files in fuzz/fuzz_targets/");
    for (target, harness_name, _, _) in TARGETS {
        let source = read(&format!("fuzz/fuzz_targets/{target}.rs"));
        let call = format!("fuzz_support::{harness_name}(data)");
        assert!(
            source.contains(&call),
            "fuzz/fuzz_targets/{target}.rs must call {call}"
        );
    }
}

#[test]
fn ci_fuzz_job_runs_every_target_from_seeds_and_keeps_reproducers() {
    let ci = read(".github/workflows/ci.yml");
    let fuzz_job = ci
        .split("\n  fuzz:")
        .nth(1)
        .expect("ci.yml has a `fuzz:` job");
    for needle in [
        "install cargo-fuzz --locked",
        "persist-credentials: false",
        "cargo +nightly fuzz run",
        "fuzz/seeds/",
        "-dict=",
        "-max_total_time=120",
        "if: failure()",
        "actions/upload-artifact@",
        "fuzz/artifacts/",
    ] {
        assert!(
            fuzz_job.contains(needle),
            "ci.yml fuzz job must contain `{needle}`"
        );
    }
    for (target, ..) in TARGETS {
        assert!(
            fuzz_job.contains(target),
            "ci.yml fuzz job does not run {target}"
        );
    }
}
