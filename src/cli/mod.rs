//! CLI entry point for the `airbnb` binary.
//!
//! `run()` is the production entry point — parses args, initializes tracing
//! to stderr, builds the client via `application::build_client`, dispatches
//! to the matching subcommand, and returns the process exit code: 0 on
//! success (also when the reader closes stdout early), 1 on a runtime
//! failure, 2 on invalid input. With `--json`, errors are JSON on stdout,
//! including the argument errors clap reports before dispatch.
//!
//! `dispatch()` is the testable variant — it takes an already-built client
//! and returns the rendered output as a `String` so integration tests can
//! assert against it without capturing stdout. Every analytical command
//! calls `application::analytical_handlers`, the orchestration the MCP
//! server shares, so both front ends make the same upstream calls.

pub mod args;
pub mod handlers;
pub mod output;

use std::ffi::OsString;
use std::io::IsTerminal;
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser as _;
use tracing_subscriber::EnvFilter;

use crate::domain::listing_id::validate_listing_id;
use crate::domain::search_params::SearchParams;
use crate::error::AirbnbError;
use crate::ports::airbnb_client::AirbnbClient;

pub use args::{Cli, Commands};

/// Exit code for success, including a reader that closed stdout early.
pub const EXIT_OK: u8 = 0;
/// Exit code for a runtime failure (network, upstream, parsing, config).
pub const EXIT_FAILURE: u8 = 1;
/// Exit code for invalid input (same as clap's usage errors).
pub const EXIT_INVALID_INPUT: u8 = 2;

/// Production entry point invoked by `src/bin/cli.rs`.
///
/// Exit codes: [`EXIT_OK`], [`EXIT_INVALID_INPUT`], [`EXIT_FAILURE`]. With
/// `--json`, a failure is printed to stdout as
/// `{"error":{"kind":…,"message":…}}` so pipelines always get JSON. That
/// includes clap's argument errors (out-of-range values, missing arguments,
/// conflicts), reported as `invalid_params` by [`usage_error_report`];
/// `--help` and `--version` stay plain text.
pub async fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(e) => match usage_error_report(&e, json_requested(&args)) {
            Some(report) => return emit(&report.output, report.to_stdout, report.exit_code),
            // Text mode, --help and --version: clap prints and exits as usual.
            None => e.exit(),
        },
    };
    init_tracing(cli.verbose);
    let as_json = cli.json;
    let (text, to_stdout, code) = match load_and_dispatch(cli).await {
        Ok(rendered) => (rendered, true, EXIT_OK),
        Err(err) => {
            let report = render_error(&err, as_json);
            (report.output, report.to_stdout, report.exit_code)
        }
    };
    emit(&text, to_stdout, code)
}

/// Write `text` to stdout or stderr and turn `code` into the process exit code.
fn emit(text: &str, to_stdout: bool, code: u8) -> ExitCode {
    let written = if to_stdout {
        output::write_output(&mut std::io::stdout().lock(), text)
    } else {
        output::write_output(&mut std::io::stderr().lock(), text)
    };
    match written {
        Ok(()) => ExitCode::from(code),
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "airbnb: failed to write output: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Whether the raw command line asks for `--json`, for reporting a usage
/// error that clap raised before `Cli::json` could be read. Recognises
/// `--json`, `--json=…`, and `-j` alone or in a cluster of short flags
/// (`-vj`). The value of `--config`/`-c` is skipped, and scanning stops at
/// `--`. The first element is the program name.
pub fn json_requested<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> bool {
    let mut rest = args.iter().skip(1).map(|a| a.as_ref().to_string_lossy());
    while let Some(arg) = rest.next() {
        if arg == "--" {
            return false;
        }
        if arg == "--json" || arg.starts_with("--json=") {
            return true;
        }
        if arg == "--config" {
            rest.next();
            continue;
        }
        if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.starts_with('-')) {
            for (i, flag) in cluster.char_indices() {
                match flag {
                    'j' => return true,
                    'v' | 'h' | 'V' => {}
                    // `-c` takes a value: the rest of the cluster, or the next argument.
                    'c' => {
                        if i + 1 == cluster.len() {
                            rest.next();
                        }
                        break;
                    }
                    _ => break,
                }
            }
        }
    }
    false
}

/// A clap usage error as a JSON [`ErrorReport`] when `as_json` is set:
/// kind `invalid_params`, exit code [`EXIT_INVALID_INPUT`], the first line of
/// clap's message without its `error: ` prefix. `None` for text mode and for
/// `--help`/`--version`, which clap prints itself.
pub fn usage_error_report(e: &clap::Error, as_json: bool) -> Option<ErrorReport> {
    if !as_json || !e.use_stderr() {
        return None;
    }
    let rendered = e.render().to_string();
    let first = rendered.lines().next().unwrap_or_default();
    let reason = first.strip_prefix("error: ").unwrap_or(first).trim();
    let err = anyhow::Error::from(AirbnbError::InvalidParams {
        reason: reason.to_string(),
    });
    Some(render_error(&err, true))
}

async fn load_and_dispatch(cli: Cli) -> Result<String> {
    let config = crate::application::load_app_config(cli.config.clone())?;
    let client = crate::application::build_client(config)?;
    dispatch(cli, client).await
}

/// How a failed command is reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    /// Text to print: a JSON document in `--json` mode, `Error: …` otherwise.
    pub output: String,
    /// `true` when `output` goes to stdout (`--json`), `false` for stderr.
    pub to_stdout: bool,
    /// Process exit code.
    pub exit_code: u8,
}

/// Turn a failure into the text to print and the exit code.
pub fn render_error(err: &anyhow::Error, as_json: bool) -> ErrorReport {
    let kind = err
        .downcast_ref::<AirbnbError>()
        .map_or("internal", error_kind);
    let exit_code = if kind == "invalid_params" {
        EXIT_INVALID_INPUT
    } else {
        EXIT_FAILURE
    };
    let message = format!("{err:#}");
    if as_json {
        let doc = serde_json::json!({ "error": { "kind": kind, "message": message } });
        let body = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| doc.to_string());
        ErrorReport {
            output: format!("{body}\n"),
            to_stdout: true,
            exit_code,
        }
    } else {
        ErrorReport {
            output: format!("Error: {}\n", output::sanitize_for_terminal(&message)),
            to_stdout: false,
            exit_code,
        }
    }
}

/// Stable, machine-readable kind of an error (the `kind` of `--json` errors).
fn error_kind(e: &AirbnbError) -> &'static str {
    match e {
        AirbnbError::InvalidParams { .. } => "invalid_params",
        // P1b: a business listing has no host section, so the host profile does not exist.
        AirbnbError::ListingNotFound { .. } | AirbnbError::HostProfileUnavailable { .. } => {
            "not_found"
        }
        AirbnbError::RateLimited => "rate_limited",
        AirbnbError::UpstreamSchema { .. } => "upstream_schema",
        AirbnbError::Http { .. } | AirbnbError::UpstreamStatus { .. } => "http",
        AirbnbError::Parse { .. } => "parse",
        AirbnbError::InsufficientData { .. } => "insufficient_data",
        AirbnbError::Config(_) | AirbnbError::Yaml(_) => "config",
        AirbnbError::AllSourcesFailed { primary, .. } => error_kind(primary),
        _ => "internal",
    }
}

/// Testable dispatcher: runs the parsed command against the given client
/// and returns the output the CLI would have printed.
#[allow(clippy::too_many_lines)] // One match arm per subcommand is the clearest shape.
pub async fn dispatch(cli: Cli, client: Arc<dyn AirbnbClient>) -> Result<String> {
    let json = cli.json;
    let out = match cli.command {
        // ---- Data tools ----
        Commands::Search(args) => {
            let params = handlers::search_args_to_params(args);
            params.validate()?;
            output::render(&client.search_listings(&params).await?, json)
        }
        Commands::Listing(args) => {
            validate_listing_id(&args.id)?;
            output::render(&client.get_listing_detail(&args.id).await?, json)
        }
        Commands::Reviews(args) => {
            validate_listing_id(&args.id)?;
            if let Some(ref cursor) = args.cursor {
                crate::domain::limits::check_cursor(cursor)?;
            }
            output::render(
                &client.get_reviews(&args.id, args.cursor.as_deref()).await?,
                json,
            )
        }
        Commands::Calendar(args) => {
            validate_listing_id(&args.id)?;
            output::render(
                &client.get_price_calendar(&args.id, args.months).await?,
                json,
            )
        }
        Commands::Host(args) => {
            validate_listing_id(&args.id)?;
            output::render(&client.get_host_profile(&args.id).await?, json)
        }
        Commands::Neighborhood(args) => {
            let sp = SearchParams {
                location: args.location,
                ..SearchParams::default()
            };
            sp.validate()?;
            output::render(&client.get_neighborhood_stats(&sp).await?, json)
        }
        Commands::Occupancy(args) => {
            validate_listing_id(&args.id)?;
            output::render(
                &client.get_occupancy_estimate(&args.id, args.months).await?,
                json,
            )
        }

        // ---- Analytical tools (application layer, shared with MCP) ----
        Commands::PriceTrends(args) => {
            let result = crate::application::analytical_handlers::run_price_trends(
                Arc::clone(&client),
                &args.id,
                args.months,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::GapFinder(args) => {
            let result = crate::application::analytical_handlers::run_gap_finder(
                Arc::clone(&client),
                &args.id,
                args.months,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::ListingScore(args) => {
            let result = crate::application::analytical_handlers::run_listing_score(
                Arc::clone(&client),
                &args.id,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::ReviewSentiment(args) => {
            let result = crate::application::analytical_handlers::run_review_sentiment(
                Arc::clone(&client),
                &args.id,
                args.max_pages,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::MarketComparison(args) => {
            let request = crate::application::analytical_handlers::MarketRequest {
                locations: args.locations,
                checkin: args.checkin,
                checkout: args.checkout,
                property_type: args.property_type,
            };
            let result = crate::application::analytical_handlers::run_market_comparison(
                Arc::clone(&client),
                request,
            )
            .await?;
            output::render(&result, json)
        }

        Commands::Compare(args) => {
            use crate::application::analytical_handlers::{CompareSearch, CompareTarget};
            let target = match (args.ids, args.location) {
                (Some(ids), _) => CompareTarget::Ids(ids),
                (None, Some(location)) => CompareTarget::Location(CompareSearch {
                    location,
                    max_listings: args
                        .max_listings
                        .unwrap_or(crate::domain::limits::COMPARE_LISTINGS_DEFAULT),
                    checkin: args.checkin,
                    checkout: args.checkout,
                    property_type: args.property_type,
                }),
                (None, None) => {
                    return Err(crate::error::AirbnbError::InvalidParams {
                        reason: "provide either --ids or --location".into(),
                    }
                    .into());
                }
            };
            let result = crate::application::analytical_handlers::run_compare_listings(
                Arc::clone(&client),
                target,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::Revenue(args) => {
            let result = crate::application::analytical_handlers::run_revenue_estimate(
                Arc::clone(&client),
                args.id.as_deref(),
                args.location,
                args.months,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::Amenities(args) => {
            let result = crate::application::analytical_handlers::run_amenity_analysis(
                Arc::clone(&client),
                &args.id,
                args.location,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::HostPortfolio(args) => {
            let result = crate::application::analytical_handlers::run_host_portfolio(
                Arc::clone(&client),
                &args.id,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::Competitive(args) => {
            let result = crate::application::analytical_handlers::run_competitive_positioning(
                Arc::clone(&client),
                &args.id,
                args.location,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::OptimalPricing(args) => {
            let result = crate::application::analytical_handlers::run_optimal_pricing(
                Arc::clone(&client),
                &args.id,
                args.location,
                args.months,
            )
            .await?;
            output::render(&result, json)
        }
    };
    Ok(out)
}

fn init_tracing(verbosity: u8) {
    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AirbnbError;

    #[test]
    fn json_errors_are_json_on_stdout_with_exit_code_2_for_invalid_input() {
        let err = anyhow::Error::from(AirbnbError::InvalidParams {
            reason: "listing id 'abc' must contain only ASCII digits".into(),
        });
        let report = render_error(&err, true);
        assert!(report.to_stdout);
        assert_eq!(report.exit_code, EXIT_INVALID_INPUT);
        let parsed: serde_json::Value = serde_json::from_str(report.output.trim()).unwrap();
        assert_eq!(parsed["error"]["kind"], "invalid_params");
        assert!(
            parsed["error"]["message"]
                .as_str()
                .unwrap()
                .contains("ASCII digits")
        );
    }

    #[test]
    fn json_requested_finds_the_flag_wherever_clap_accepts_it() {
        let yes: [&[&str]; 6] = [
            &["airbnb", "--json", "listing", "1"],
            &["airbnb", "listing", "1", "--json"],
            &["airbnb", "-j", "listing", "1"],
            &["airbnb", "-vj", "listing", "1"],
            &["airbnb", "-c", "cfg.yaml", "-j", "listing", "1"],
            &["airbnb", "--config", "-j", "--json", "listing", "1"],
        ];
        for args in yes {
            assert!(json_requested(args), "{args:?}");
        }
        let no: [&[&str]; 6] = [
            &["airbnb", "listing", "1"],
            &["--json"],
            &["airbnb", "--config", "--json", "listing", "1"],
            &["airbnb", "-c", "-j", "listing", "1"],
            &["airbnb", "-cj", "listing", "1"],
            &["airbnb", "search", "--", "--json"],
        ];
        for args in no {
            assert!(!json_requested(args), "{args:?}");
        }
    }

    #[test]
    fn usage_errors_become_invalid_params_json_in_json_mode() {
        let e = Cli::try_parse_from(["airbnb", "--json", "price-trends", "42", "--months", "13"])
            .unwrap_err();
        let report = usage_error_report(&e, true).expect("a JSON report");
        assert!(report.to_stdout);
        assert_eq!(report.exit_code, EXIT_INVALID_INPUT);
        let parsed: serde_json::Value = serde_json::from_str(report.output.trim()).unwrap();
        assert_eq!(parsed["error"]["kind"], "invalid_params");
        let message = parsed["error"]["message"].as_str().unwrap();
        assert!(
            message.starts_with("Invalid parameters: invalid value '13' for '--months"),
            "{message}"
        );
        assert!(
            usage_error_report(&e, false).is_none(),
            "text mode stays clap's"
        );
    }

    #[test]
    fn help_and_version_are_never_json_errors() {
        for flag in ["--help", "--version"] {
            let e = Cli::try_parse_from(["airbnb", "--json", flag]).unwrap_err();
            assert!(usage_error_report(&e, true).is_none(), "{flag}");
        }
    }

    #[test]
    fn text_errors_go_to_stderr_with_exit_code_1() {
        let err = anyhow::Error::from(AirbnbError::RateLimited);
        let report = render_error(&err, false);
        assert!(!report.to_stdout);
        assert_eq!(report.exit_code, EXIT_FAILURE);
        assert!(report.output.starts_with("Error: "), "{}", report.output);
    }

    #[test]
    fn untyped_errors_are_internal() {
        let report = render_error(&anyhow::anyhow!("boom"), true);
        let parsed: serde_json::Value = serde_json::from_str(report.output.trim()).unwrap();
        assert_eq!(parsed["error"]["kind"], "internal");
        assert_eq!(report.exit_code, EXIT_FAILURE);
    }

    #[test]
    fn text_errors_are_sanitized() {
        let report = render_error(&anyhow::anyhow!("bad \u{1b}[2J input"), false);
        assert!(!report.output.contains('\u{1b}'));
    }

    #[test]
    fn error_kinds_follow_the_p2_p3_error_taxonomy() {
        let cases = [
            (
                AirbnbError::Http {
                    reason: "timeout".into(),
                },
                "http",
            ),
            (
                AirbnbError::UpstreamStatus {
                    status: 503,
                    context: "search".into(),
                },
                "http",
            ),
            (
                AirbnbError::InsufficientData {
                    reason: "no nightly price".into(),
                },
                "insufficient_data",
            ),
            (
                AirbnbError::HostProfileUnavailable {
                    listing_id: "42".into(),
                    reason: "business listing (pdpType HOTEL)".into(),
                },
                "not_found",
            ),
            (
                AirbnbError::AllSourcesFailed {
                    primary: Box::new(AirbnbError::RateLimited),
                    fallback: Box::new(AirbnbError::Parse {
                        reason: "no data".into(),
                    }),
                },
                "rate_limited",
            ),
        ];
        for (err, kind) in cases {
            let report = render_error(&anyhow::Error::from(err), true);
            let parsed: serde_json::Value = serde_json::from_str(report.output.trim()).unwrap();
            assert_eq!(parsed["error"]["kind"], kind);
            assert_eq!(report.exit_code, EXIT_FAILURE);
        }
    }
}
