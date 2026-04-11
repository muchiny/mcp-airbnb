//! Clap derive definitions for the `airbnb` CLI.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "airbnb",
    version,
    about = "Query Airbnb listings and analytics from your terminal",
    long_about = None,
)]
pub struct Cli {
    /// Path to config.yaml (overrides auto-discovery and `AIRBNB_CONFIG` env var)
    #[arg(long, short = 'c', global = true, env = "AIRBNB_CONFIG")]
    pub config: Option<PathBuf>,

    /// Output machine-readable JSON instead of human-readable text
    #[arg(long, short = 'j', global = true)]
    pub json: bool,

    /// Increase logging verbosity (-v: info, -vv: debug)
    #[arg(long, short = 'v', global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    // ---- Data tools (7) ----
    /// Search listings by location
    Search(SearchArgs),
    /// Fetch full details for a listing
    Listing(IdArgs),
    /// Fetch guest reviews for a listing
    Reviews(ReviewsArgs),
    /// Fetch price calendar for a listing
    Calendar(CalendarArgs),
    /// Fetch the host profile associated with a listing
    Host(IdArgs),
    /// Fetch aggregated neighborhood statistics
    Neighborhood(LocationArgs),
    /// Estimate occupancy for a listing
    Occupancy(CalendarArgs),

    // ---- Analytical tools (11) ----
    /// Compare multiple listings side-by-side
    Compare(CompareArgs),
    /// Analyze seasonal price trends from the calendar
    PriceTrends(CalendarArgs),
    /// Detect booking gaps and orphan nights
    GapFinder(CalendarArgs),
    /// Estimate revenue (ADR, occupancy, monthly/annual)
    Revenue(IdMonthsLocationArgs),
    /// Score a listing's quality (0–100)
    ListingScore(IdArgs),
    /// Compare a listing's amenities vs its neighborhood
    Amenities(IdLocationArgs),
    /// Compare 2–5 neighborhoods side-by-side
    MarketComparison(MarketComparisonArgs),
    /// Analyze a host's full property portfolio
    HostPortfolio(IdArgs),
    /// Analyze sentiment in a listing's guest reviews
    ReviewSentiment(ReviewSentimentArgs),
    /// Evaluate competitive positioning across 5 axes
    Competitive(IdLocationArgs),
    /// Recommend optimal pricing based on market data
    OptimalPricing(IdMonthsLocationArgs),
}

// ---------------- Argument structs ----------------

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Location to search (e.g. "Paris, France")
    pub location: String,

    /// Check-in date (YYYY-MM-DD)
    #[arg(long)]
    pub checkin: Option<String>,

    /// Check-out date (YYYY-MM-DD)
    #[arg(long)]
    pub checkout: Option<String>,

    /// Number of adult guests
    #[arg(long, default_value_t = 1)]
    pub adults: u32,

    /// Number of children
    #[arg(long)]
    pub children: Option<u32>,

    /// Number of infants
    #[arg(long)]
    pub infants: Option<u32>,

    /// Number of pets
    #[arg(long)]
    pub pets: Option<u32>,

    /// Minimum price per night
    #[arg(long)]
    pub min_price: Option<u32>,

    /// Maximum price per night
    #[arg(long)]
    pub max_price: Option<u32>,

    /// Property type filter (e.g. "Entire home")
    #[arg(long)]
    pub property_type: Option<String>,

    /// Pagination cursor from a previous search result
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Debug, Args)]
pub struct IdArgs {
    /// Listing ID (or host listing ID)
    pub id: String,
}

#[derive(Debug, Args)]
pub struct LocationArgs {
    /// Location (e.g. "Barcelona, Spain")
    pub location: String,
}

#[derive(Debug, Args)]
pub struct ReviewsArgs {
    /// Listing ID
    pub id: String,

    /// Pagination cursor from a previous page
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Debug, Args)]
pub struct CalendarArgs {
    /// Listing ID
    pub id: String,

    /// Number of months to fetch (1–12)
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=12))]
    pub months: u32,
}

#[derive(Debug, Args)]
pub struct IdLocationArgs {
    /// Listing ID
    pub id: String,

    /// Optional location override (defaults to the listing's own location)
    #[arg(long)]
    pub location: Option<String>,
}

#[derive(Debug, Args)]
pub struct IdMonthsLocationArgs {
    /// Listing ID
    pub id: String,

    /// Optional location override (defaults to the listing's own location)
    #[arg(long)]
    pub location: Option<String>,

    /// Months of calendar data to use (1–12)
    #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=12))]
    pub months: u32,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    /// Comma-separated list of listing IDs (mutually exclusive with --location)
    #[arg(long, value_delimiter = ',', conflicts_with = "location")]
    pub ids: Option<Vec<String>>,

    /// Location to discover listings via search (mutually exclusive with --ids)
    #[arg(long)]
    pub location: Option<String>,
}

#[derive(Debug, Args)]
pub struct MarketComparisonArgs {
    /// 2–5 locations to compare (comma-separated or repeated). Validated
    /// at dispatch time rather than parse time because `num_args` interacts
    /// awkwardly with `value_delimiter` for positional args in clap 4.
    #[arg(value_delimiter = ',', num_args = 1..)]
    pub locations: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ReviewSentimentArgs {
    /// Listing ID
    pub id: String,

    /// Maximum number of review pages to fetch (1–20)
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=20))]
    pub max_pages: u32,
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn parse_search_basic() {
        let cli = parse(&["airbnb", "search", "Paris"]).unwrap();
        match cli.command {
            Commands::Search(args) => {
                assert_eq!(args.location, "Paris");
                assert_eq!(args.adults, 1);
                assert!(args.checkin.is_none());
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn parse_search_with_all_filters() {
        let cli = parse(&[
            "airbnb",
            "--json",
            "search",
            "Barcelona",
            "--checkin",
            "2025-06-01",
            "--checkout",
            "2025-06-08",
            "--adults",
            "2",
            "--max-price",
            "200",
            "--property-type",
            "Entire home",
        ])
        .unwrap();
        assert!(cli.json);
        match cli.command {
            Commands::Search(args) => {
                assert_eq!(args.location, "Barcelona");
                assert_eq!(args.checkin.as_deref(), Some("2025-06-01"));
                assert_eq!(args.checkout.as_deref(), Some("2025-06-08"));
                assert_eq!(args.adults, 2);
                assert_eq!(args.max_price, Some(200));
                assert_eq!(args.property_type.as_deref(), Some("Entire home"));
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn parse_calendar_months_range_rejects_zero() {
        assert!(parse(&["airbnb", "calendar", "12345", "--months", "0"]).is_err());
    }

    #[test]
    fn parse_calendar_months_range_rejects_thirteen() {
        assert!(parse(&["airbnb", "calendar", "12345", "--months", "13"]).is_err());
    }

    #[test]
    fn parse_calendar_months_accepts_valid_range() {
        let cli = parse(&["airbnb", "calendar", "12345", "--months", "6"]).unwrap();
        match cli.command {
            Commands::Calendar(args) => {
                assert_eq!(args.id, "12345");
                assert_eq!(args.months, 6);
            }
            _ => panic!("expected Calendar"),
        }
    }

    #[test]
    fn parse_global_config_flag() {
        let cli = parse(&["airbnb", "--config", "/tmp/test.yaml", "listing", "abc123"]).unwrap();
        assert_eq!(
            cli.config.as_deref().unwrap().to_str().unwrap(),
            "/tmp/test.yaml"
        );
    }

    #[test]
    fn parse_verbose_count() {
        let cli = parse(&["airbnb", "-vv", "listing", "42"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn parse_compare_ids() {
        let cli = parse(&["airbnb", "compare", "--ids", "1,2,3"]).unwrap();
        match cli.command {
            Commands::Compare(args) => {
                assert_eq!(
                    args.ids.as_deref(),
                    Some(&["1".to_string(), "2".to_string(), "3".to_string()][..])
                );
                assert!(args.location.is_none());
            }
            _ => panic!("expected Compare"),
        }
    }

    #[test]
    fn parse_compare_location() {
        let cli = parse(&["airbnb", "compare", "--location", "Rome"]).unwrap();
        match cli.command {
            Commands::Compare(args) => {
                assert!(args.ids.is_none());
                assert_eq!(args.location.as_deref(), Some("Rome"));
            }
            _ => panic!("expected Compare"),
        }
    }

    #[test]
    fn parse_compare_ids_and_location_conflict() {
        assert!(
            parse(&["airbnb", "compare", "--ids", "1,2", "--location", "Rome",]).is_err(),
            "--ids and --location should be mutually exclusive",
        );
    }

    #[test]
    fn parse_market_comparison_accepts_two_locations() {
        let cli = parse(&["airbnb", "market-comparison", "Paris,Lyon"]).unwrap();
        match cli.command {
            Commands::MarketComparison(args) => {
                assert_eq!(
                    args.locations,
                    vec!["Paris".to_string(), "Lyon".to_string()]
                );
            }
            _ => panic!("expected MarketComparison"),
        }
    }

    #[test]
    fn parse_market_comparison_accepts_single_location_at_parse_time() {
        // Parse-time accepts; runtime validation in dispatch rejects < 2.
        let cli = parse(&["airbnb", "market-comparison", "Paris"]).unwrap();
        match cli.command {
            Commands::MarketComparison(args) => assert_eq!(args.locations.len(), 1),
            _ => panic!("expected MarketComparison"),
        }
    }

    #[test]
    fn parse_market_comparison_accepts_six_at_parse_time() {
        // Parse-time accepts; runtime validation in dispatch rejects > 5.
        let cli = parse(&["airbnb", "market-comparison", "A,B,C,D,E,F"]).unwrap();
        match cli.command {
            Commands::MarketComparison(args) => assert_eq!(args.locations.len(), 6),
            _ => panic!("expected MarketComparison"),
        }
    }

    #[test]
    fn parse_id_location_defaults_to_none() {
        let cli = parse(&["airbnb", "amenities", "12345"]).unwrap();
        match cli.command {
            Commands::Amenities(args) => {
                assert_eq!(args.id, "12345");
                assert!(args.location.is_none());
            }
            _ => panic!("expected Amenities"),
        }
    }

    #[test]
    fn parse_revenue_with_months_and_location() {
        let cli = parse(&[
            "airbnb",
            "revenue",
            "12345",
            "--location",
            "Tokyo",
            "--months",
            "6",
        ])
        .unwrap();
        match cli.command {
            Commands::Revenue(args) => {
                assert_eq!(args.id, "12345");
                assert_eq!(args.location.as_deref(), Some("Tokyo"));
                assert_eq!(args.months, 6);
            }
            _ => panic!("expected Revenue"),
        }
    }

    #[test]
    fn parse_review_sentiment_max_pages_range() {
        assert!(parse(&["airbnb", "review-sentiment", "12345", "--max-pages", "0",]).is_err());
        assert!(parse(&["airbnb", "review-sentiment", "12345", "--max-pages", "21",]).is_err());
    }

    #[test]
    fn verify_cli_debug_assert() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
