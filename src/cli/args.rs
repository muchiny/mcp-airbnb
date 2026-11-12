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
    /// Path to config.yaml; the file must exist (also read from `AIRBNB_CONFIG`).
    /// Without it: ~/.config/mcp-airbnb/config.yaml, then config.yaml next to
    /// the binary, then built-in defaults. The working directory is not searched.
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
    // ---- Data tools ----
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

    // ---- Analytical tools ----
    /// Compare multiple listings side-by-side
    Compare(CompareArgs),
    /// Analyze seasonal price trends from the calendar
    PriceTrends(PriceTrendsArgs),
    /// Detect booking gaps and orphan nights
    GapFinder(CalendarArgs),
    /// Estimate revenue (ADR, occupancy, monthly/annual)
    Revenue(RevenueArgs),
    /// Score a listing's quality (0–100)
    ListingScore(IdArgs),
    /// Compare a listing's amenities vs its neighborhood
    Amenities(IdLocationArgs),
    /// Compare 2–5 neighborhoods side-by-side
    MarketComparison(MarketComparisonArgs),
    /// List a host's properties visible in a search of the listing's city
    HostPortfolio(IdArgs),
    /// Analyze sentiment in a listing's guest reviews
    ReviewSentiment(ReviewSentimentArgs),
    /// Rank a listing against comparable listings (price, rating, amenities, reviews)
    Competitive(IdLocationArgs),
    /// Recommend optimal pricing based on market data
    OptimalPricing(IdMonthsLocationArgs),
    /// Find where a listing ranks in the search results of a location
    Rank(RankArgs),

    // ---- Dated price and diagnostics ----
    /// Quote the real price of a stay (total, per night, availability)
    Quote(QuoteArgs),
    /// Check every Airbnb source this tool depends on
    #[command(alias = "doctor")]
    Status(StatusArgs),
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

    /// Number of adult guests (1–16). If omitted, no guest count is sent
    /// and Airbnb applies its own default (same as the MCP tool).
    #[arg(long)]
    pub adults: Option<u32>,

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

    /// Property type filter: "Entire home", "Private room" or "Hotel room"
    #[arg(long)]
    pub property_type: Option<String>,

    /// Minimum number of bedrooms (1–16)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub min_bedrooms: Option<u32>,

    /// Minimum number of beds (1–16)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub min_beds: Option<u32>,

    /// Minimum number of bathrooms (1–16)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub min_bathrooms: Option<u32>,

    /// Amenity every result must have (repeatable): wifi, air conditioning, pool,
    /// kitchen, free parking, hot tub, heating, washer, dryer, dedicated workspace,
    /// self check-in, tv, bbq grill, crib
    #[arg(long = "amenity", value_name = "NAME")]
    pub amenities: Vec<String>,

    /// Only Guest Favorite listings
    #[arg(long)]
    pub guest_favorite: bool,

    /// Only listings that can be booked instantly
    #[arg(long)]
    pub instant_book: bool,

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
    /// Stay and filters of the comparable search
    #[command(flatten)]
    pub stay: StayArgs,
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
    /// Stay and filters of the comparable search
    #[command(flatten)]
    pub stay: StayArgs,
}

/// Stay and comparable filters shared by the comparable-based commands.
#[derive(Debug, Args, Default)]
pub struct StayArgs {
    /// Check-in date (YYYY-MM-DD) of the stay comparables are searched for; needs --checkout
    #[arg(long)]
    pub checkin: Option<String>,

    /// Check-out date (YYYY-MM-DD); needs --checkin
    #[arg(long)]
    pub checkout: Option<String>,

    /// Number of adults of the stay (1–16)
    #[arg(long)]
    pub adults: Option<u32>,

    /// Keep only comparables of this type: "Entire home", "Private room" or "Hotel room"
    #[arg(long)]
    pub property_type: Option<String>,

    /// Keep only comparables with at least this many bedrooms (1–16)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub min_bedrooms: Option<u32>,
}

#[derive(Debug, Args)]
pub struct QuoteArgs {
    /// Listing ID
    pub id: String,

    /// Check-in date (YYYY-MM-DD)
    #[arg(long)]
    pub checkin: String,

    /// Check-out date (YYYY-MM-DD)
    #[arg(long)]
    pub checkout: String,

    /// Number of adults (1–16)
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub adults: u32,

    /// Weekly stays to quote (1–12): the same stay again, one week later each time
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=12))]
    pub weeks: u32,
}

#[derive(Debug, Args)]
pub struct RankArgs {
    /// Listing ID to locate
    pub id: String,

    /// Location to search (defaults to the listing's own location)
    #[arg(long)]
    pub location: Option<String>,

    /// Stay and filters of the search
    #[command(flatten)]
    pub stay: StayArgs,

    /// Number of children
    #[arg(long)]
    pub children: Option<u32>,

    /// Amenity every result must have (repeatable; same names as `search --amenity`)
    #[arg(long = "amenity", value_name = "NAME")]
    pub amenities: Vec<String>,

    /// Search pages to scan at most (1–15, 18 results each)
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=15))]
    pub max_pages: u32,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Location the checks search (default: "Paris, France")
    #[arg(long)]
    pub location: Option<String>,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    /// Comma-separated list of 2–10 listing IDs (mutually exclusive with --location)
    #[arg(long, value_delimiter = ',', conflicts_with = "location")]
    pub ids: Option<Vec<String>>,

    /// Location to discover listings via search (mutually exclusive with --ids)
    #[arg(long)]
    pub location: Option<String>,

    /// Listings to compare in --location mode (2–100, default 20, same as the MCP tool)
    #[arg(long, requires = "location", conflicts_with = "ids", value_parser = clap::value_parser!(u32).range(2..=100))]
    pub max_listings: Option<u32>,

    /// Check-in date (YYYY-MM-DD) for --location mode
    #[arg(long, requires = "location", conflicts_with = "ids")]
    pub checkin: Option<String>,

    /// Check-out date (YYYY-MM-DD) for --location mode
    #[arg(long, requires = "location", conflicts_with = "ids")]
    pub checkout: Option<String>,

    /// Property type filter for --location mode (e.g. "Entire home")
    #[arg(long, requires = "location", conflicts_with = "ids")]
    pub property_type: Option<String>,
}

#[derive(Debug, Args)]
pub struct PriceTrendsArgs {
    /// Listing ID
    pub id: String,

    /// Number of months to analyze (1–12, default 12, same as the MCP tool)
    #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=12))]
    pub months: u32,
}

#[derive(Debug, Args)]
pub struct RevenueArgs {
    /// Listing ID (omit it and pass --location for a market-level estimate)
    #[arg(required_unless_present = "location")]
    pub id: Option<String>,

    /// Location: overrides the listing's own location, or is required without an ID
    #[arg(long)]
    pub location: Option<String>,

    /// Months of calendar data to use (1–12)
    #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=12))]
    pub months: u32,
    /// Stay and filters of the comparable search
    #[command(flatten)]
    pub stay: StayArgs,
}

#[derive(Debug, Args)]
pub struct MarketComparisonArgs {
    /// 2–5 locations, one per argument. Quote a location that contains
    /// spaces or commas: `"Paris, France" "Lyon, France"`
    #[arg(required = true, num_args = 2..=5, value_parser = parse_location)]
    pub locations: Vec<String>,

    /// Check-in date (YYYY-MM-DD); must be paired with --checkout
    #[arg(long)]
    pub checkin: Option<String>,

    /// Check-out date (YYYY-MM-DD); must be paired with --checkin
    #[arg(long)]
    pub checkout: Option<String>,

    /// Property type filter (e.g. "Entire home")
    #[arg(long)]
    pub property_type: Option<String>,
}

/// clap value parser for one location argument: trimmed, non-empty and at
/// most `limits::LOCATION_MAX_CHARS` characters.
fn parse_location(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    crate::domain::limits::check_location(trimmed).map_err(|e| e.to_string())?;
    Ok(trimmed.to_string())
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
    fn parse_search_panel_filters() {
        let cli = parse(&[
            "airbnb",
            "search",
            "Porto-Vecchio",
            "--min-bedrooms",
            "2",
            "--min-beds",
            "3",
            "--min-bathrooms",
            "1",
            "--amenity",
            "pool",
            "--amenity",
            "air conditioning",
            "--guest-favorite",
            "--instant-book",
        ])
        .unwrap();
        let Commands::Search(args) = cli.command else {
            panic!("expected Search");
        };
        assert_eq!(args.min_bedrooms, Some(2));
        assert_eq!(args.min_beds, Some(3));
        assert_eq!(args.min_bathrooms, Some(1));
        assert_eq!(args.amenities, ["pool", "air conditioning"]);
        assert!(args.guest_favorite && args.instant_book);
        assert!(parse(&["airbnb", "search", "Paris", "--min-bedrooms", "0"]).is_err());
        assert!(parse(&["airbnb", "search", "Paris", "--min-beds", "17"]).is_err());
    }

    #[test]
    fn parse_quote_with_defaults_and_ranges() {
        let cli = parse(&[
            "airbnb",
            "quote",
            "42",
            "--checkin",
            "2027-07-10",
            "--checkout",
            "2027-07-17",
        ])
        .unwrap();
        let Commands::Quote(args) = cli.command else {
            panic!("expected Quote");
        };
        assert_eq!(args.id, "42");
        assert_eq!((args.adults, args.weeks), (1, 1));
        assert_eq!(args.checkout, "2027-07-17");

        let base = ["airbnb", "quote", "42", "--checkin", "2027-07-10"];
        assert!(parse(&base).is_err(), "checkout is required");
        let dated = [
            "airbnb",
            "quote",
            "42",
            "--checkin",
            "2027-07-10",
            "--checkout",
            "2027-07-17",
        ];
        for extra in [
            ["--weeks", "0"],
            ["--weeks", "13"],
            ["--adults", "0"],
            ["--adults", "17"],
        ] {
            let args: Vec<&str> = dated.iter().chain(extra.iter()).copied().collect();
            assert!(parse(&args).is_err(), "{extra:?}");
        }
        let args: Vec<&str> = dated
            .iter()
            .chain(["--weeks", "12"].iter())
            .copied()
            .collect();
        assert!(parse(&args).is_ok());
    }

    #[test]
    fn parse_rank_with_stay_and_filters() {
        let cli = parse(&[
            "airbnb",
            "rank",
            "42",
            "--location",
            "Porto-Vecchio",
            "--checkin",
            "2027-07-10",
            "--checkout",
            "2027-07-17",
            "--adults",
            "4",
            "--children",
            "1",
            "--property-type",
            "Entire home",
            "--min-bedrooms",
            "2",
            "--amenity",
            "pool",
            "--max-pages",
            "15",
        ])
        .unwrap();
        let Commands::Rank(args) = cli.command else {
            panic!("expected Rank");
        };
        assert_eq!(args.id, "42");
        assert_eq!(args.location.as_deref(), Some("Porto-Vecchio"));
        assert_eq!(args.stay.adults, Some(4));
        assert_eq!(args.stay.min_bedrooms, Some(2));
        assert_eq!(args.children, Some(1));
        assert_eq!(args.amenities, ["pool"]);
        assert_eq!(args.max_pages, 15);

        let Commands::Rank(defaults) = parse(&["airbnb", "rank", "42"]).unwrap().command else {
            panic!("expected Rank");
        };
        assert_eq!(defaults.max_pages, 5);
        assert!(defaults.location.is_none());
        assert!(parse(&["airbnb", "rank", "42", "--max-pages", "16"]).is_err());
        assert!(parse(&["airbnb", "rank", "42", "--max-pages", "0"]).is_err());
    }

    #[test]
    fn parse_status_and_its_doctor_alias() {
        for name in ["status", "doctor"] {
            let Commands::Status(args) = parse(&["airbnb", name]).unwrap().command else {
                panic!("expected Status for {name}");
            };
            assert!(args.location.is_none());
        }
        let Commands::Status(args) = parse(&["airbnb", "status", "--location", "Lyon, France"])
            .unwrap()
            .command
        else {
            panic!("expected Status");
        };
        assert_eq!(args.location.as_deref(), Some("Lyon, France"));
    }

    #[test]
    fn parse_stay_options_of_the_comparable_based_commands() {
        let Commands::OptimalPricing(args) = parse(&[
            "airbnb",
            "optimal-pricing",
            "42",
            "--checkin",
            "2027-07-10",
            "--checkout",
            "2027-07-17",
            "--adults",
            "4",
            "--property-type",
            "Entire home",
            "--min-bedrooms",
            "2",
        ])
        .unwrap()
        .command
        else {
            panic!("expected OptimalPricing");
        };
        assert_eq!(args.stay.checkin.as_deref(), Some("2027-07-10"));
        assert_eq!(args.stay.adults, Some(4));
        assert_eq!(args.stay.property_type.as_deref(), Some("Entire home"));
        assert_eq!(args.stay.min_bedrooms, Some(2));

        let Commands::Competitive(args) = parse(&["airbnb", "competitive", "42"]).unwrap().command
        else {
            panic!("expected Competitive");
        };
        assert!(args.stay.checkin.is_none() && args.stay.min_bedrooms.is_none());
        let Commands::Revenue(args) = parse(&[
            "airbnb",
            "revenue",
            "--location",
            "Rome",
            "--min-bedrooms",
            "3",
        ])
        .unwrap()
        .command
        else {
            panic!("expected Revenue");
        };
        assert_eq!(args.stay.min_bedrooms, Some(3));
        assert!(parse(&["airbnb", "amenities", "42", "--min-bedrooms", "17"]).is_err());
    }

    #[test]
    fn parse_search_basic() {
        let cli = parse(&["airbnb", "search", "Paris"]).unwrap();
        match cli.command {
            Commands::Search(args) => {
                assert_eq!(args.location, "Paris");
                assert_eq!(args.adults, None);
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
                assert_eq!(args.adults, Some(2));
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
    fn parse_market_comparison_keeps_city_country_pairs() {
        let cli = parse(&[
            "airbnb",
            "market-comparison",
            "Paris, France",
            "Barcelona, Spain",
            "Rome, Italy",
        ])
        .unwrap();
        match cli.command {
            Commands::MarketComparison(args) => assert_eq!(
                args.locations,
                vec![
                    "Paris, France".to_string(),
                    "Barcelona, Spain".to_string(),
                    "Rome, Italy".to_string()
                ]
            ),
            _ => panic!("expected MarketComparison"),
        }
    }

    #[test]
    fn parse_market_comparison_rejects_one_comma_joined_argument() {
        assert!(parse(&["airbnb", "market-comparison", "Paris,Lyon"]).is_err());
    }

    #[test]
    fn parse_market_comparison_rejects_one_and_six_locations() {
        assert!(parse(&["airbnb", "market-comparison", "Paris"]).is_err());
        assert!(parse(&["airbnb", "market-comparison", "A", "B", "C", "D", "E", "F"]).is_err());
    }

    #[test]
    fn parse_market_comparison_trims_and_rejects_blank_locations() {
        let cli = parse(&["airbnb", "market-comparison", "  Paris ", "Lyon"]).unwrap();
        match cli.command {
            Commands::MarketComparison(args) => assert_eq!(args.locations[0], "Paris"),
            _ => panic!("expected MarketComparison"),
        }
        assert!(parse(&["airbnb", "market-comparison", "Paris", "   "]).is_err());
    }

    #[test]
    fn parse_market_comparison_accepts_filters() {
        let cli = parse(&[
            "airbnb",
            "market-comparison",
            "Paris",
            "Lyon",
            "--checkin",
            "2026-10-01",
            "--checkout",
            "2026-10-05",
            "--property-type",
            "Entire home",
        ])
        .unwrap();
        match cli.command {
            Commands::MarketComparison(args) => {
                assert_eq!(args.checkin.as_deref(), Some("2026-10-01"));
                assert_eq!(args.property_type.as_deref(), Some("Entire home"));
            }
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
                assert_eq!(args.id.as_deref(), Some("12345"));
                assert_eq!(args.location.as_deref(), Some("Tokyo"));
                assert_eq!(args.months, 6);
            }
            _ => panic!("expected Revenue"),
        }
    }

    #[test]
    fn parse_price_trends_defaults_to_twelve_months() {
        let cli = parse(&["airbnb", "price-trends", "12345"]).unwrap();
        match cli.command {
            Commands::PriceTrends(args) => assert_eq!(args.months, 12),
            _ => panic!("expected PriceTrends"),
        }
    }

    #[test]
    fn parse_revenue_location_only() {
        let cli = parse(&["airbnb", "revenue", "--location", "Rome, Italy"]).unwrap();
        match cli.command {
            Commands::Revenue(args) => {
                assert!(args.id.is_none());
                assert_eq!(args.location.as_deref(), Some("Rome, Italy"));
            }
            _ => panic!("expected Revenue"),
        }
        assert!(parse(&["airbnb", "revenue"]).is_err());
    }

    #[test]
    fn parse_compare_location_filters() {
        let cli = parse(&[
            "airbnb",
            "compare",
            "--location",
            "Rome",
            "--max-listings",
            "40",
            "--property-type",
            "Entire home",
        ])
        .unwrap();
        match cli.command {
            Commands::Compare(args) => {
                assert_eq!(args.max_listings, Some(40));
                assert_eq!(args.property_type.as_deref(), Some("Entire home"));
            }
            _ => panic!("expected Compare"),
        }
        assert!(parse(&["airbnb", "compare", "--ids", "1,2", "--max-listings", "40"]).is_err());
        assert!(
            parse(&[
                "airbnb",
                "compare",
                "--location",
                "Rome",
                "--max-listings",
                "101"
            ])
            .is_err()
        );
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
