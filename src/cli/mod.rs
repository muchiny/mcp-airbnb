//! CLI entry point for the `airbnb` binary.
//!
//! `run()` is the production entry point — parses args, initializes tracing
//! to stderr, builds the client via `application::build_client`, and
//! dispatches to the matching subcommand.
//!
//! `dispatch()` is the testable variant — it takes an already-built client
//! and returns the rendered output as a `String` so integration tests can
//! assert against it without capturing stdout.

pub mod args;
pub mod handlers;
pub mod output;

use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::domain::analytics::{
    compute_gap_finder, compute_listing_score, compute_market_comparison, compute_price_trends,
    compute_review_sentiment,
};
use crate::domain::search_params::SearchParams;
use crate::ports::airbnb_client::AirbnbClient;

pub use args::{Cli, Commands};

/// Production entry point invoked by `src/bin/cli.rs`.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(crate::application::find_config_path);
    let config = crate::config::load_config(&config_path)?;
    let client = crate::application::build_client(config)?;

    let rendered = dispatch(cli, client).await?;
    print!("{rendered}");
    Ok(())
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
            output::render(&client.get_listing_detail(&args.id).await?, json)
        }
        Commands::Reviews(args) => output::render(
            &client.get_reviews(&args.id, args.cursor.as_deref()).await?,
            json,
        ),
        Commands::Calendar(args) => output::render(
            &client.get_price_calendar(&args.id, args.months).await?,
            json,
        ),
        Commands::Host(args) => output::render(&client.get_host_profile(&args.id).await?, json),
        Commands::Neighborhood(args) => {
            let sp = SearchParams {
                location: args.location,
                ..SearchParams::default()
            };
            output::render(&client.get_neighborhood_stats(&sp).await?, json)
        }
        Commands::Occupancy(args) => output::render(
            &client.get_occupancy_estimate(&args.id, args.months).await?,
            json,
        ),

        // ---- Analytical: single-fetch (inline) ----
        Commands::PriceTrends(args) => {
            let calendar = client.get_price_calendar(&args.id, args.months).await?;
            output::render(&compute_price_trends(&args.id, &calendar), json)
        }
        Commands::GapFinder(args) => {
            let calendar = client.get_price_calendar(&args.id, args.months).await?;
            output::render(&compute_gap_finder(&args.id, &calendar), json)
        }
        Commands::ListingScore(args) => {
            let detail = client.get_listing_detail(&args.id).await?;
            // Try to get neighborhood stats for pricing comparison; ignore errors.
            let sp = SearchParams {
                location: detail.location.clone(),
                ..SearchParams::default()
            };
            let neighborhood = client.get_neighborhood_stats(&sp).await.ok();
            output::render(&compute_listing_score(&detail, neighborhood.as_ref()), json)
        }
        Commands::ReviewSentiment(args) => {
            let mut all_reviews = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..args.max_pages {
                match client.get_reviews(&args.id, cursor.as_deref()).await {
                    Ok(page) => {
                        all_reviews.extend(page.reviews);
                        cursor = page.next_cursor;
                        if cursor.is_none() {
                            break;
                        }
                    }
                    Err(e) => {
                        if all_reviews.is_empty() {
                            return Err(e.into());
                        }
                        break;
                    }
                }
            }
            output::render(&compute_review_sentiment(&args.id, &all_reviews), json)
        }
        Commands::MarketComparison(args) => {
            if !(2..=5).contains(&args.locations.len()) {
                return Err(anyhow::anyhow!(
                    "market-comparison requires 2–5 locations, got {}",
                    args.locations.len()
                ));
            }
            let mut stats = Vec::with_capacity(args.locations.len());
            for location in args.locations {
                let sp = SearchParams {
                    location,
                    ..SearchParams::default()
                };
                stats.push(client.get_neighborhood_stats(&sp).await?);
            }
            output::render(&compute_market_comparison(&stats), json)
        }

        // ---- Analytical: multi-fetch (application layer) ----
        Commands::Compare(args) => {
            let result = crate::application::analytical_handlers::run_compare_listings(
                Arc::clone(&client),
                args.ids,
                args.location,
            )
            .await?;
            output::render(&result, json)
        }
        Commands::Revenue(args) => {
            let result = crate::application::analytical_handlers::run_revenue_estimate(
                Arc::clone(&client),
                &args.id,
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
