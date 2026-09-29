#![allow(clippy::cast_precision_loss)] // Counts are small enough for f64

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use super::calendar::{CalendarDay, DayStatus, PriceCalendar};
use super::listing::{Listing, ListingDetail};
use super::review::Review;
use crate::error::AirbnbError;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProfile {
    pub host_id: Option<String>,
    pub name: String,
    pub is_superhost: Option<bool>,
    pub response_rate: Option<String>,
    pub response_time: Option<String>,
    pub member_since: Option<String>,
    pub languages: Vec<String>,
    pub total_listings: Option<u32>,
    pub description: Option<String>,
    pub profile_picture_url: Option<String>,
    pub identity_verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTypeCount {
    pub property_type: String,
    pub count: u32,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborhoodStats {
    pub location: String,
    pub total_listings: u32,
    pub average_price: Option<f64>,
    pub median_price: Option<f64>,
    pub price_range: Option<(f64, f64)>,
    pub average_rating: Option<f64>,
    pub property_type_distribution: Vec<PropertyTypeCount>,
    /// Share (0-100) of all listings flagged superhost. `None` when no
    /// listing reports the flag at all.
    pub superhost_percentage: Option<f64>,
    /// Currency of the price statistics: the most common currency among
    /// priced listings. `None` when no listing is priced.
    #[serde(default)]
    pub currency: Option<String>,
    /// Listings whose known price (in `currency`) feeds the price statistics.
    #[serde(default)]
    pub priced_listings: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyOccupancy {
    pub month: String,
    pub total_days: u32,
    pub occupied_days: u32,
    pub available_days: u32,
    pub occupancy_rate: f64,
    pub average_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OccupancyEstimate {
    pub listing_id: String,
    /// First future night considered (`YYYY-MM-DD`), empty when none.
    pub period_start: String,
    /// Last future night considered (`YYYY-MM-DD`), empty when none.
    pub period_end: String,
    /// Nights considered: open + unavailable. Past and host-blocked days are
    /// excluded.
    pub total_days: u32,
    /// Unavailable nights (booked, or blocked for a reason Airbnb does not
    /// disclose).
    pub occupied_days: u32,
    pub available_days: u32,
    /// `occupied_days / total_days × 100`, an upper bound. `0.0` when
    /// `total_days == 0` (use [`OccupancyEstimate::measured_rate`]).
    pub occupancy_rate: f64,
    /// Days before the reference date, left out of every figure.
    #[serde(default)]
    pub past_days_excluded: u32,
    /// Host-blocked or stay-rule days, left out of every figure.
    #[serde(default)]
    pub blocked_days_excluded: u32,
    /// Currency of the calendar prices.
    #[serde(default)]
    pub currency: String,
    pub average_available_price: Option<f64>,
    pub weekend_avg_price: Option<f64>,
    pub weekday_avg_price: Option<f64>,
    pub monthly_breakdown: Vec<MonthlyOccupancy>,
}

impl OccupancyEstimate {
    /// The occupancy rate, only when at least one future night was measured.
    pub fn measured_rate(&self) -> Option<f64> {
        (self.total_days > 0).then_some(self.occupancy_rate)
    }
}

// ---------------------------------------------------------------------------
// Compare Listings types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingComparison {
    pub id: String,
    pub name: String,
    pub price_per_night: f64,
    pub currency: String,
    pub rating: Option<f64>,
    pub review_count: u32,
    pub property_type: Option<String>,
    pub is_superhost: Option<bool>,
    pub bedrooms: Option<u32>,
    pub amenities_count: Option<u32>,
    /// Mid-rank percentile of the price among priced listings in the same
    /// currency (0-100, lower = cheaper). `None` when the price is unknown.
    pub price_percentile: Option<f64>,
    /// Mid-rank percentile of the rating (0-100, higher = better).
    pub rating_percentile: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub count: u32,
    /// Listings with a known price in `currency`.
    #[serde(default)]
    pub priced_count: u32,
    /// Currency of the price statistics.
    #[serde(default)]
    pub currency: Option<String>,
    pub avg_price: Option<f64>,
    pub median_price: Option<f64>,
    pub avg_rating: Option<f64>,
    pub price_range: Option<(f64, f64)>,
    pub superhost_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareListingsResult {
    pub listings: Vec<ListingComparison>,
    pub summary: ComparisonSummary,
}

// ---------------------------------------------------------------------------
// Price Trends types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyPriceSummary {
    pub month: String,
    /// `None` when no open night of the month has a published price.
    pub avg_price: Option<f64>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub weekend_avg: Option<f64>,
    pub weekday_avg: Option<f64>,
    /// Open future nights of the month.
    pub available_days: u32,
    /// Future nights of the month (past days excluded).
    pub total_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayOfWeekPrice {
    pub day: String,
    pub avg_price: f64,
    pub sample_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceTrends {
    pub listing_id: String,
    pub currency: String,
    pub period_start: String,
    pub period_end: String,
    /// `None` when no open night has a published price.
    pub overall_avg: Option<f64>,
    pub overall_min: Option<f64>,
    pub overall_max: Option<f64>,
    /// Standard deviation / mean (coefficient of variation). `None` with
    /// fewer than two priced nights.
    pub price_volatility: Option<f64>,
    /// `(weekend_avg - weekday_avg) / weekday_avg * 100`
    pub weekend_premium_pct: Option<f64>,
    pub peak_month: Option<String>,
    pub off_peak_month: Option<String>,
    pub monthly: Vec<MonthlyPriceSummary>,
    pub day_of_week: Vec<DayOfWeekPrice>,
    /// Open future nights that carried a published price.
    #[serde(default)]
    pub priced_nights: u32,
}

// ---------------------------------------------------------------------------
// Gap Finder types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarGap {
    pub start_date: String,
    pub end_date: String,
    pub nights: u32,
    pub avg_price: Option<f64>,
    pub potential_revenue: Option<f64>,
    /// Either `orphan` (1 night) or `short_gap` (2-3 nights).
    pub gap_type: String,
    /// Minimum stay required for a check-in on the gap's first night.
    #[serde(default)]
    pub min_nights: Option<u32>,
    /// `Some(false)` when the gap is shorter than that minimum stay, so it
    /// cannot be booked as-is.
    #[serde(default)]
    pub bookable_as_is: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapFinderResult {
    pub listing_id: String,
    /// Currency of the calendar prices.
    #[serde(default)]
    pub currency: String,
    pub total_gaps: u32,
    pub total_gap_nights: u32,
    pub orphan_nights: u32,
    pub short_gaps: u32,
    /// Gaps shorter than the minimum stay of their first night.
    #[serde(default)]
    pub unbookable_gaps: u32,
    /// Sum of `nights × average price` over gaps with a published price.
    /// `None` when no gap has one.
    pub potential_lost_revenue: Option<f64>,
    pub gaps: Vec<CalendarGap>,
    /// Length of the shortest unbookable gap. Lowering the minimum stay to
    /// this many nights on gap check-in dates would let those gaps sell.
    /// Never higher than the current minimum.
    pub suggested_min_nights: Option<u32>,
}

// ---------------------------------------------------------------------------
// Revenue Estimate types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyRevenue {
    pub month: String,
    pub projected_revenue: f64,
    pub projected_occupancy_pct: f64,
    pub avg_nightly_rate: f64,
}

/// Where an estimate input comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Measured from the listing's own calendar.
    Calendar,
    /// The listing's own known nightly price (for example from a dated search).
    ListingPrice,
    /// Average nightly price of priced comparable listings (a proxy).
    NeighborhoodAverage,
    /// Not measured: a fixed assumption, always labelled as such.
    #[default]
    Assumption,
}

/// Occupancy assumed for location-only estimates (no calendar). It is an
/// industry rule of thumb, always printed as an assumption.
pub const ASSUMED_OCCUPANCY_PCT: f64 = 65.0;

/// Average nights per month (365.25 / 12).
const NIGHTS_PER_MONTH: f64 = 30.44;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueEstimate {
    pub listing_id: Option<String>,
    pub location: String,
    pub projected_adr: f64,
    /// Where `projected_adr` comes from.
    #[serde(default)]
    pub adr_source: DataSource,
    pub projected_occupancy_pct: f64,
    /// `Calendar` when measured, `Assumption` for location-only estimates.
    #[serde(default)]
    pub occupancy_source: DataSource,
    /// Future calendar nights behind a measured occupancy (0 when assumed).
    #[serde(default)]
    pub occupancy_nights_measured: u32,
    pub projected_monthly_revenue: f64,
    pub projected_annual_revenue: f64,
    pub vs_neighborhood_avg_price_pct: Option<f64>,
    pub currency: String,
    pub monthly_breakdown: Vec<MonthlyRevenue>,
}

// ---------------------------------------------------------------------------
// Listing Score types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryScore {
    pub category: String,
    /// 0-100. `None` when the category cannot be scored (missing data), in
    /// which case it is left out of the overall score.
    pub score: Option<f64>,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingScore {
    pub listing_id: String,
    pub overall_score: f64,
    pub category_scores: Vec<CategoryScore>,
    pub suggestions: Vec<String>,
}

// ---------------------------------------------------------------------------
// Amenity Analysis types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmenityGap {
    pub amenity: String,
    pub neighborhood_frequency_pct: f64,
    pub is_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmenityAnalysis {
    pub listing_id: String,
    /// Distinct amenities after normalization.
    pub listing_amenity_count: u32,
    /// Mean distinct-amenity count of the analyzed comparables (0 when none).
    pub neighborhood_avg_amenity_count: f64,
    pub missing_popular_amenities: Vec<AmenityGap>,
    pub present_rare_amenities: Vec<AmenityGap>,
    /// Listing count / comparable average × 100 (capped at 200). `None` when
    /// no comparable listing with amenity data was analyzed.
    pub amenity_score_pct: Option<f64>,
    /// Comparable listings with at least one amenity that were analyzed.
    #[serde(default)]
    pub comparables_analyzed: u32,
    /// Distinct-amenity count of each analyzed comparable.
    #[serde(default)]
    pub neighbor_amenity_counts: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Market Comparison types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub location: String,
    pub total_listings: u32,
    pub avg_price: Option<f64>,
    pub median_price: Option<f64>,
    pub avg_rating: Option<f64>,
    pub superhost_pct: Option<f64>,
    pub top_property_type: Option<String>,
    /// Currency of this market's prices.
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketComparison {
    pub locations: Vec<MarketSnapshot>,
}

// ---------------------------------------------------------------------------
// Host Portfolio types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioProperty {
    pub id: String,
    pub name: String,
    pub location: String,
    /// Known nightly price. `None` when Airbnb showed no price.
    pub price_per_night: Option<f64>,
    /// Currency of `price_per_night`.
    #[serde(default)]
    pub currency: String,
    pub rating: Option<f64>,
    pub review_count: u32,
    pub property_type: Option<String>,
}

/// How the other properties were attributed to the queried listing's host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostMatch {
    /// The properties share the queried listing's host id.
    HostId,
    /// The properties share only the host display name (first names are
    /// not unique).
    HostName,
    /// No other property identified: only the queried listing.
    #[default]
    AnchorOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPortfolio {
    pub host_name: String,
    pub host_id: Option<String>,
    pub total_properties: u32,
    pub avg_rating: Option<f64>,
    /// Mean known price in `currency`. `None` when no property is priced.
    pub avg_price: Option<f64>,
    pub price_range: Option<(f64, f64)>,
    /// Currency of the price statistics.
    #[serde(default)]
    pub currency: Option<String>,
    pub total_reviews: u32,
    pub is_superhost: Option<bool>,
    /// How the other properties were attributed to this host.
    #[serde(default)]
    pub matched_by: HostMatch,
    /// Location whose first search page was examined.
    #[serde(default)]
    pub search_location: String,
    /// Airbnb's own listing count for the host, when the detail shows it.
    #[serde(default)]
    pub host_reported_listings: Option<u32>,
    pub properties: Vec<PortfolioProperty>,
}

// ---------------------------------------------------------------------------
// Review Sentiment types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTheme {
    pub theme: String,
    pub mention_count: u32,
    pub positive_count: u32,
    pub negative_count: u32,
    pub sample_quotes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSentiment {
    pub listing_id: String,
    pub total_reviews_analyzed: u32,
    pub positive_pct: f64,
    pub negative_pct: f64,
    pub neutral_pct: f64,
    pub themes: Vec<ReviewTheme>,
    pub top_positive_keywords: Vec<(String, u32)>,
    pub top_negative_keywords: Vec<(String, u32)>,
    /// Reviews left out because they are not in English (and not translated).
    #[serde(default)]
    pub skipped_non_english: u32,
}

// ---------------------------------------------------------------------------
// Competitive Positioning types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitiveAxis {
    pub axis: String,
    /// The listing's own value on this axis, when known.
    pub listing_value: Option<f64>,
    /// Mean of the comparable listings on this axis, when known.
    pub neighborhood_avg: Option<f64>,
    /// Share (0-100) of comparable listings this listing beats on this axis
    /// (mid-rank percentile rank). `None` when it cannot be ranked.
    pub percentile: Option<f64>,
    /// Comparable listings behind `percentile`.
    #[serde(default)]
    pub sample_size: u32,
    pub assessment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitivePositioning {
    pub listing_id: String,
    pub axes: Vec<CompetitiveAxis>,
    /// Mean of the ranked axes. `None` when no axis could be ranked.
    pub overall_competitiveness: Option<f64>,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
}

// ---------------------------------------------------------------------------
// Optimal Pricing types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRecommendation {
    pub listing_id: String,
    /// The listing's own known nightly price. `None` when Airbnb showed none.
    pub current_price: Option<f64>,
    /// Currency of `current_price` (the listing's own). It can differ from
    /// `currency` when the neighborhood median is quoted in another one.
    #[serde(default)]
    pub current_price_currency: String,
    pub recommended_price: f64,
    pub recommended_range: (f64, f64),
    pub currency: String,
    pub reasoning: Vec<String>,
    pub weekday_recommendation: Option<f64>,
    pub weekend_recommendation: Option<f64>,
    pub amenity_premium_pct: Option<f64>,
    pub vs_neighborhood_median: Option<f64>,
}

// ---------------------------------------------------------------------------
// Display impls
// ---------------------------------------------------------------------------

impl std::fmt::Display for HostProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Host: {}", self.name)?;
        if let Some(ref id) = self.host_id {
            writeln!(f, "ID: {id}")?;
        }
        if self.is_superhost == Some(true) {
            writeln!(f, "Superhost: Yes")?;
        }
        if let Some(ref rate) = self.response_rate {
            writeln!(f, "Response rate: {rate}")?;
        }
        if let Some(ref time) = self.response_time {
            writeln!(f, "Response time: {time}")?;
        }
        if let Some(ref since) = self.member_since {
            writeln!(f, "Member since: {since}")?;
        }
        if !self.languages.is_empty() {
            writeln!(f, "Languages: {}", self.languages.join(", "))?;
        }
        if let Some(count) = self.total_listings {
            writeln!(f, "Total listings: {count}")?;
        }
        if self.identity_verified == Some(true) {
            writeln!(f, "Identity verified: Yes")?;
        }
        if let Some(ref desc) = self.description {
            writeln!(f, "\n{desc}")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for NeighborhoodStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Neighborhood: {}", self.location)?;
        writeln!(f, "Listings analyzed: {}", self.total_listings)?;
        let currency = self.currency.as_deref();
        if let Some(avg) = self.average_price {
            if self.priced_listings > 0 {
                writeln!(
                    f,
                    "Priced listings: {} of {}",
                    self.priced_listings, self.total_listings
                )?;
            }
            writeln!(f, "Average price: {}/night", money(currency, avg))?;
        } else {
            writeln!(
                f,
                "Prices: unavailable (no listing in this search shows a nightly price)"
            )?;
        }
        if let Some(median) = self.median_price {
            writeln!(f, "Median price: {}/night", money(currency, median))?;
        }
        if let Some((min, max)) = self.price_range {
            writeln!(
                f,
                "Price range: {} - {}/night",
                money(currency, min),
                money(currency, max)
            )?;
        }
        if let Some(rating) = self.average_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        match self.superhost_percentage {
            Some(pct) => writeln!(f, "Superhosts (flagged): {pct:.0}%")?,
            None => writeln!(
                f,
                "Superhosts: unknown (no listing reports superhost status)"
            )?,
        }
        if !self.property_type_distribution.is_empty() {
            writeln!(f, "\nProperty types:")?;
            for pt in &self.property_type_distribution {
                writeln!(
                    f,
                    "  {} — {} ({:.0}%)",
                    pt.property_type, pt.count, pt.percentage
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for OccupancyEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Occupancy: listing {}", self.listing_id)?;
        if self.total_days == 0 {
            writeln!(
                f,
                "Occupancy rate: unknown (the calendar window has no future nights to measure)"
            )?;
        } else {
            writeln!(f, "Period: {} to {}", self.period_start, self.period_end)?;
            writeln!(
                f,
                "Nights considered: {} ({} unavailable, {} open)",
                self.total_days, self.occupied_days, self.available_days
            )?;
            writeln!(
                f,
                "Occupancy rate: {:.1}% (upper bound: Airbnb does not distinguish booked nights from host-blocked nights)",
                self.occupancy_rate
            )?;
        }
        if self.past_days_excluded > 0 {
            writeln!(f, "Past days excluded: {}", self.past_days_excluded)?;
        }
        if self.blocked_days_excluded > 0 {
            writeln!(
                f,
                "Host-blocked days excluded: {}",
                self.blocked_days_excluded
            )?;
        }
        let currency = Some(self.currency.as_str());
        if self.average_available_price.is_none()
            && self.weekend_avg_price.is_none()
            && self.weekday_avg_price.is_none()
        {
            writeln!(f, "Prices: not published by Airbnb for this calendar")?;
        } else {
            if let Some(avg) = self.average_available_price {
                writeln!(f, "Avg open-night price: {}/night", money(currency, avg))?;
            }
            if let Some(weekend) = self.weekend_avg_price {
                writeln!(
                    f,
                    "Weekend avg (Fri-Sat): {}/night",
                    money(currency, weekend)
                )?;
            }
            if let Some(weekday) = self.weekday_avg_price {
                writeln!(f, "Weekday avg: {}/night", money(currency, weekday))?;
            }
        }
        if !self.monthly_breakdown.is_empty() {
            writeln!(f, "\nMonthly breakdown (future nights only):")?;
            writeln!(
                f,
                "{:<10} {:>6} {:>8} {:>8} {:>10} {:>10}",
                "Month", "Nights", "Unavail", "Open", "Occ%", "Avg price"
            )?;
            for m in &self.monthly_breakdown {
                let price = m
                    .average_price
                    .map_or_else(|| "-".to_string(), |p| money(currency, p));
                writeln!(
                    f,
                    "{:<10} {:>6} {:>8} {:>8} {:>9.1}% {:>10}",
                    m.month,
                    m.total_days,
                    m.occupied_days,
                    m.available_days,
                    m.occupancy_rate,
                    price
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for CompareListingsResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "# Listing Comparison ({} listings)\n",
            self.summary.count
        )?;
        let currency = self.summary.currency.as_deref();
        if let (Some(avg), Some(median), Some((low, high))) = (
            self.summary.avg_price,
            self.summary.median_price,
            self.summary.price_range,
        ) {
            writeln!(
                f,
                "Summary: avg {}/night, median {}/night, range {}-{} ({} of {} listings priced)",
                money(currency, avg),
                money(currency, median),
                money(currency, low),
                money(currency, high),
                self.summary.priced_count,
                self.summary.count
            )?;
        } else {
            writeln!(f, "Summary: no listing has a known nightly price")?;
        }
        if let Some(rating) = self.summary.avg_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        writeln!(f, "Superhosts: {}\n", self.summary.superhost_count)?;
        writeln!(
            f,
            "{:<8} {:<30} {:>10} {:>8} {:>10} {:>12} {:>10}",
            "ID", "Name", "Price", "Rating", "Reviews", "Type", "Price%"
        )?;
        writeln!(f, "{}", "-".repeat(94))?;
        for l in &self.listings {
            let rating = l
                .rating
                .map_or_else(|| "-".to_string(), |r| format!("{r:.1}"));
            let ptype = l
                .property_type
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(12)
                .collect::<String>();
            let name: String = l.name.chars().take(28).collect();
            let price = if l.price_per_night.is_finite() && l.price_per_night > 0.0 {
                money(Some(l.currency.as_str()), l.price_per_night)
            } else {
                "n/a".to_string()
            };
            let percentile = l
                .price_percentile
                .map_or_else(|| "-".to_string(), |p| format!("{p:.0}%"));
            writeln!(
                f,
                "{:<8} {:<30} {:>10} {:>8} {:>10} {:>12} {:>10}",
                l.id, name, price, rating, l.review_count, ptype, percentile,
            )?;
        }
        Ok(())
    }
}

impl std::fmt::Display for PriceTrends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let currency = Some(self.currency.as_str());
        let cell =
            |value: Option<f64>| value.map_or_else(|| "-".to_string(), |v| money(currency, v));
        writeln!(f, "# Price Trends: listing {}", self.listing_id)?;
        writeln!(f, "Period: {} to {}", self.period_start, self.period_end)?;
        if let (Some(avg), Some(min), Some(max)) =
            (self.overall_avg, self.overall_min, self.overall_max)
        {
            writeln!(
                f,
                "Overall: avg {}, min {}, max {} ({} priced open nights)",
                money(currency, avg),
                money(currency, min),
                money(currency, max),
                self.priced_nights
            )?;
        } else {
            writeln!(
                f,
                "Nightly prices: not published — Airbnb's calendar returned no price for any open night, so price trends cannot be computed (availability is shown below)"
            )?;
        }
        if let Some(volatility) = self.price_volatility {
            writeln!(f, "Price volatility: {:.1}%", volatility * 100.0)?;
        }
        if let Some(premium) = self.weekend_premium_pct {
            writeln!(f, "Weekend premium: {premium:+.1}%")?;
        }
        if let Some(ref peak) = self.peak_month {
            writeln!(f, "Peak month: {peak}")?;
        }
        if let Some(ref off_peak) = self.off_peak_month {
            writeln!(f, "Off-peak month: {off_peak}")?;
        }
        if !self.monthly.is_empty() {
            writeln!(f, "\nMonthly breakdown (future nights only):")?;
            writeln!(
                f,
                "{:<10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>6}/{:>6}",
                "Month", "Avg", "Min", "Max", "WE avg", "WD avg", "Open", "Total"
            )?;
            for m in &self.monthly {
                writeln!(
                    f,
                    "{:<10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>6}/{:>6}",
                    m.month,
                    cell(m.avg_price),
                    cell(m.min_price),
                    cell(m.max_price),
                    cell(m.weekend_avg),
                    cell(m.weekday_avg),
                    m.available_days,
                    m.total_days,
                )?;
            }
        }
        if !self.day_of_week.is_empty() {
            writeln!(f, "\nDay-of-week averages:")?;
            for d in &self.day_of_week {
                writeln!(
                    f,
                    "  {:<10} {} ({} nights)",
                    d.day,
                    money(currency, d.avg_price),
                    d.sample_count
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for GapFinderResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Gap Analysis: listing {}", self.listing_id)?;
        writeln!(
            f,
            "Found {} gaps of 1-3 open nights between unavailable nights ({} total gap nights)",
            self.total_gaps, self.total_gap_nights
        )?;
        writeln!(f, "Orphan nights (1-night gaps): {}", self.orphan_nights)?;
        writeln!(f, "Short gaps (2-3 nights): {}", self.short_gaps)?;
        writeln!(
            f,
            "Gaps shorter than the minimum stay (unbookable as-is): {}",
            self.unbookable_gaps
        )?;
        let currency = Some(self.currency.as_str());
        match self.potential_lost_revenue {
            Some(revenue) => writeln!(
                f,
                "Potential revenue of gap nights: {} (at calendar prices, if every gap night sold)",
                money(currency, revenue)
            )?,
            None if self.total_gaps > 0 => writeln!(
                f,
                "Potential revenue of gap nights: unknown (Airbnb's calendar does not publish nightly prices)"
            )?,
            None => {}
        }
        if let Some(min) = self.suggested_min_nights {
            writeln!(
                f,
                "Suggestion: lower the minimum stay to {min} night(s) on gap check-in dates so the {} unbookable gap(s) can sell",
                self.unbookable_gaps
            )?;
        }
        if !self.gaps.is_empty() {
            writeln!(f, "\nGaps:")?;
            for g in &self.gaps {
                let min_stay = match (g.min_nights, g.bookable_as_is) {
                    (Some(min), Some(false)) => format!(", min stay {min}: unbookable as-is"),
                    (Some(min), _) => format!(", min stay {min}"),
                    (None, _) => String::new(),
                };
                let revenue = g.potential_revenue.map_or_else(String::new, |r| {
                    format!(" ({} potential)", money(currency, r))
                });
                writeln!(
                    f,
                    "  {} to {} — {} night(s) [{}{min_stay}]{revenue}",
                    g.start_date, g.end_date, g.nights, g.gap_type
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for RevenueEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Revenue Estimate")?;
        if let Some(ref id) = self.listing_id {
            writeln!(f, "Listing: {id}")?;
        }
        writeln!(f, "Location: {}", self.location)?;
        let currency = Some(self.currency.as_str());
        let adr_note = match self.adr_source {
            DataSource::Calendar => "measured: average of the listing's open-night calendar prices",
            DataSource::ListingPrice => "measured: the listing's own nightly price",
            DataSource::NeighborhoodAverage => {
                "PROXY: average nightly price of comparable listings, not this listing's own price"
            }
            DataSource::Assumption => "ASSUMPTION",
        };
        writeln!(
            f,
            "Projected ADR: {}/night ({adr_note})",
            money(currency, self.projected_adr)
        )?;
        if self.occupancy_source == DataSource::Calendar {
            writeln!(
                f,
                "Projected occupancy: {:.1}% (measured over {} future calendar nights; upper bound, because Airbnb does not distinguish bookings from host blocks)",
                self.projected_occupancy_pct, self.occupancy_nights_measured
            )?;
        } else {
            writeln!(
                f,
                "Projected occupancy: {:.1}% (ASSUMPTION: no calendar data; industry rule of thumb, not measured)",
                self.projected_occupancy_pct
            )?;
        }
        writeln!(
            f,
            "Projected monthly revenue: {} (ADR × occupancy × 30.44 nights, extrapolated)",
            money(currency, self.projected_monthly_revenue)
        )?;
        writeln!(
            f,
            "Projected annual revenue: {} (monthly × 12, extrapolated)",
            money(currency, self.projected_annual_revenue)
        )?;
        if let Some(pct) = self.vs_neighborhood_avg_price_pct {
            writeln!(f, "vs neighborhood avg price: {pct:+.1}%")?;
        }
        if !self.monthly_breakdown.is_empty() {
            writeln!(
                f,
                "\nRevenue on nights already unavailable, by month (upper bound):"
            )?;
            writeln!(
                f,
                "{:<10} {:>12} {:>10} {:>12}",
                "Month", "Revenue", "Occ%", "Rate"
            )?;
            for m in &self.monthly_breakdown {
                writeln!(
                    f,
                    "{:<10} {:>12} {:>9.1}% {:>12}",
                    m.month,
                    money(currency, m.projected_revenue),
                    m.projected_occupancy_pct,
                    money(currency, m.avg_nightly_rate),
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for ListingScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Listing Score: {}", self.listing_id)?;
        writeln!(f, "Overall score: {:.0}/100\n", self.overall_score)?;
        for cat in &self.category_scores {
            let score = cat
                .score
                .map_or_else(|| "n/a".to_string(), |s| format!("{s:.0}/100"));
            writeln!(f, "  {}: {score} — {}", cat.category, cat.details)?;
        }
        if !self.suggestions.is_empty() {
            writeln!(f, "\nSuggestions:")?;
            for s in &self.suggestions {
                writeln!(f, "  - {s}")?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for AmenityAnalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Amenity Analysis: listing {}", self.listing_id)?;
        let Some(score) = self.amenity_score_pct else {
            writeln!(f, "Your amenities: {}", self.listing_amenity_count)?;
            writeln!(
                f,
                "Amenity score: unavailable (no comparable listing with amenity data could be analyzed)"
            )?;
            return Ok(());
        };
        writeln!(
            f,
            "Your amenities: {} (average of {} comparable listings: {:.1})",
            self.listing_amenity_count,
            self.comparables_analyzed,
            self.neighborhood_avg_amenity_count
        )?;
        writeln!(f, "Amenity score: {score:.0}% of the comparable average\n")?;
        if !self.missing_popular_amenities.is_empty() {
            writeln!(f, "Missing popular amenities:")?;
            for a in &self.missing_popular_amenities {
                writeln!(
                    f,
                    "  - {} ({:.0}% of comparables have it)",
                    a.amenity, a.neighborhood_frequency_pct
                )?;
            }
        }
        if !self.present_rare_amenities.is_empty() {
            writeln!(f, "\nYour unique/rare amenities:")?;
            for a in &self.present_rare_amenities {
                writeln!(
                    f,
                    "  + {} (only {:.0}% of comparables)",
                    a.amenity, a.neighborhood_frequency_pct
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for MarketComparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "# Market Comparison ({} locations)\n",
            self.locations.len()
        )?;
        writeln!(
            f,
            "{:<25} {:>10} {:>10} {:>10} {:>8} {:>10}",
            "Location", "Listings", "Avg price", "Med price", "Rating", "SH%"
        )?;
        writeln!(f, "{}", "-".repeat(78))?;
        for loc in &self.locations {
            let currency = loc.currency.as_deref();
            let avg = loc
                .avg_price
                .map_or_else(|| "-".to_string(), |p| money(currency, p));
            let med = loc
                .median_price
                .map_or_else(|| "-".to_string(), |p| money(currency, p));
            let rating = loc
                .avg_rating
                .map_or_else(|| "-".to_string(), |r| format!("{r:.2}"));
            let sh = loc
                .superhost_pct
                .map_or_else(|| "-".to_string(), |p| format!("{p:.0}%"));
            let location: String = loc.location.chars().take(24).collect();
            writeln!(
                f,
                "{:<25} {:>10} {:>10} {:>10} {:>8} {:>10}",
                location, loc.total_listings, avg, med, rating, sh
            )?;
        }
        Ok(())
    }
}

impl std::fmt::Display for HostPortfolio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Host Portfolio: {}", self.host_name)?;
        if let Some(ref id) = self.host_id {
            writeln!(f, "Host ID: {id}")?;
        }
        if self.is_superhost == Some(true) {
            writeln!(f, "Superhost: Yes")?;
        }
        let matched = match self.matched_by {
            HostMatch::HostId => "matched by host id",
            HostMatch::HostName => {
                "matched by host display name only; other hosts with the same first name may be included"
            }
            HostMatch::AnchorOnly => "no other listing by this host was identified",
        };
        writeln!(
            f,
            "Coverage: listings found on the first search page for \"{}\" ({matched}); this is not the host's full portfolio",
            self.search_location
        )?;
        if let Some(count) = self.host_reported_listings {
            writeln!(f, "Airbnb reports {count} listing(s) for this host")?;
        }
        writeln!(f, "Properties found: {}", self.total_properties)?;
        if let Some(rating) = self.avg_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        let currency = self.currency.as_deref();
        if let (Some(avg), Some((low, high))) = (self.avg_price, self.price_range) {
            writeln!(
                f,
                "Average price: {}/night (range: {}-{})",
                money(currency, avg),
                money(currency, low),
                money(currency, high)
            )?;
        } else {
            writeln!(
                f,
                "Average price: unknown (no property shows a nightly price)"
            )?;
        }
        writeln!(f, "Total reviews: {}", self.total_reviews)?;
        if !self.properties.is_empty() {
            writeln!(f, "\nProperties:")?;
            for (i, p) in self.properties.iter().enumerate() {
                let rating = p
                    .rating
                    .map_or_else(|| "-".to_string(), |r| format!("{r:.1}"));
                let ptype = p.property_type.as_deref().unwrap_or("-");
                let price = p.price_per_night.map_or_else(
                    || "price unavailable".to_string(),
                    |v| format!("{}/night", money(Some(p.currency.as_str()), v)),
                );
                writeln!(
                    f,
                    "  {}. {} (ID: {}) — {} — {price}, {rating} ({} reviews) [{ptype}]",
                    i + 1,
                    p.name,
                    p.id,
                    p.location,
                    p.review_count,
                )?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for ReviewSentiment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "=== Review Sentiment Analysis: listing {} ===",
            self.listing_id
        )?;
        writeln!(
            f,
            "Method: English keyword matching with simple negation handling (heuristic, not a language model)"
        )?;
        writeln!(f, "Reviews Analyzed: {}", self.total_reviews_analyzed)?;
        if self.skipped_non_english > 0 {
            writeln!(
                f,
                "Skipped: {} non-English reviews (the keyword lists are English-only)",
                self.skipped_non_english
            )?;
        }
        if self.total_reviews_analyzed == 0 {
            writeln!(f, "Sentiment: not computed (no English review to analyze)")?;
        } else {
            writeln!(
                f,
                "Sentiment: {:.0}% positive, {:.0}% negative, {:.0}% neutral",
                self.positive_pct, self.negative_pct, self.neutral_pct
            )?;
        }
        if !self.themes.is_empty() {
            writeln!(f, "\n--- Themes ---")?;
            for theme in &self.themes {
                writeln!(
                    f,
                    "{}: {} mentions ({} positive, {} negative)",
                    theme.theme, theme.mention_count, theme.positive_count, theme.negative_count
                )?;
                for quote in &theme.sample_quotes {
                    writeln!(f, "  \"{quote}\"")?;
                }
            }
        }
        if !self.top_positive_keywords.is_empty() {
            writeln!(f, "\n--- Top Positive Keywords ---")?;
            for (word, count) in &self.top_positive_keywords {
                writeln!(f, "  {word} ({count})")?;
            }
        }
        if !self.top_negative_keywords.is_empty() {
            writeln!(f, "\n--- Top Negative Keywords ---")?;
            for (word, count) in &self.top_negative_keywords {
                writeln!(f, "  {word} ({count})")?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for CompetitivePositioning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "=== Competitive Positioning: listing {} ===",
            self.listing_id
        )?;
        match self.overall_competitiveness {
            Some(overall) => writeln!(
                f,
                "Overall Competitiveness: {overall:.0}/100 (mean of the ranked axes)"
            )?,
            None => writeln!(
                f,
                "Overall Competitiveness: not computable (no axis could be ranked against comparable listings)"
            )?,
        }
        if !self.axes.is_empty() {
            writeln!(f, "\n--- Axes ---")?;
            for axis in &self.axes {
                let value = axis
                    .listing_value
                    .map_or_else(|| "unknown".to_string(), |v| format!("{v:.1}"));
                let average = axis.neighborhood_avg.map_or_else(
                    || "no comparable figure".to_string(),
                    |a| format!("comparable avg: {a:.1}"),
                );
                let rank = axis.percentile.map_or_else(
                    || "not ranked".to_string(),
                    |p| format!("ranks above {p:.0}% of {} comparables", axis.sample_size),
                );
                writeln!(
                    f,
                    "{}: {value} ({average}) — {rank} — {}",
                    axis.axis, axis.assessment
                )?;
            }
        }
        if !self.strengths.is_empty() {
            writeln!(f, "\nStrengths: {}", self.strengths.join(", "))?;
        }
        if !self.weaknesses.is_empty() {
            writeln!(f, "Weaknesses: {}", self.weaknesses.join(", "))?;
        }
        Ok(())
    }
}

impl std::fmt::Display for PricingRecommendation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "=== Pricing Recommendation: listing {} ===",
            self.listing_id
        )?;
        let currency = Some(self.currency.as_str());
        match self.current_price {
            Some(price) => writeln!(
                f,
                "Current Price: {}/night",
                money2(Some(self.current_price_currency.as_str()), price)
            )?,
            None => writeln!(
                f,
                "Current Price: unknown (Airbnb shows nightly prices only for dated searches)"
            )?,
        }
        writeln!(
            f,
            "Recommended Price: {}/night",
            money2(currency, self.recommended_price)
        )?;
        writeln!(
            f,
            "Recommended Range: {} - {}/night",
            money2(currency, self.recommended_range.0),
            money2(currency, self.recommended_range.1)
        )?;
        if let (Some(weekday), Some(weekend)) =
            (self.weekday_recommendation, self.weekend_recommendation)
        {
            writeln!(
                f,
                "\nWeekday: {}  |  Weekend: {}",
                money2(currency, weekday),
                money2(currency, weekend)
            )?;
        }
        if let Some(premium) = self.amenity_premium_pct {
            writeln!(f, "Amenity adjustment: {premium:+.1}%")?;
        }
        if let Some(vs) = self.vs_neighborhood_median {
            writeln!(f, "vs Neighborhood Median: {vs:+.0}%")?;
        }
        if !self.reasoning.is_empty() {
            writeln!(f, "\nReasoning:")?;
            for reason in &self.reasoning {
                writeln!(f, "  - {reason}")?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pure computation functions
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Mean of the finite values, or `None` when there are none.
fn mean(values: &[f64]) -> Option<f64> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        None
    } else {
        Some(finite.iter().sum::<f64>() / finite.len() as f64)
    }
}

/// Guest rating as a 0-100 quality score: 4.0 or less → 0, 5.0 → 100.
fn rating_quality_score(rating: f64) -> f64 {
    ((rating - 4.0) * 100.0).clamp(0.0, 100.0)
}

/// The percentage in a host string such as `"98%"` or
/// `"Response rate: 100%"`.
fn parse_percent(text: &str) -> Option<f64> {
    let (head, _) = text.split_once('%')?;
    let reversed: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let digits: String = reversed.chars().rev().collect();
    digits
        .parse::<f64>()
        .ok()
        .filter(|pct| pct.is_finite() && (0.0..=100.0).contains(pct))
}

/// Pricing category of the listing score. `None` when the listing has no
/// known price, there is no market average, or the currencies differ.
fn pricing_category(
    detail: &ListingDetail,
    neighborhood: Option<&NeighborhoodStats>,
    suggestions: &mut Vec<String>,
) -> (Option<f64>, String) {
    let Some(price) = detail.known_price() else {
        return (
            None,
            "no known nightly price (Airbnb shows prices only for dated searches)".to_string(),
        );
    };
    let currency = Some(detail.currency.as_str());
    let Some(stats) = neighborhood else {
        return (
            None,
            format!("{}/night (no market data)", money(currency, price)),
        );
    };
    let Some(avg) = stats.average_price.filter(|a| a.is_finite() && *a > 0.0) else {
        return (
            None,
            format!("{}/night (no market average)", money(currency, price)),
        );
    };
    if stats
        .currency
        .as_deref()
        .is_some_and(|market| market != detail.currency)
    {
        return (
            None,
            format!(
                "{}/night (market prices are in another currency)",
                money(currency, price)
            ),
        );
    }
    let score = match price / avg {
        r if r < 0.5 => {
            suggestions
                .push("Your price is significantly below market — consider raising it".to_string());
            40.0
        }
        r if r < 0.8 => 70.0,
        r if r <= 1.2 => 100.0, // well-positioned
        r if r <= 1.5 => 70.0,
        _ => {
            suggestions.push("Your price is significantly above market average".to_string());
            40.0
        }
    };
    (
        Some(score),
        format!(
            "{}/night (market avg: {})",
            money(currency, price),
            money(currency, avg)
        ),
    )
}

/// Median of an ascending slice.
fn median_sorted(sorted: &[f64]) -> Option<f64> {
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n.is_multiple_of(2) => Some(f64::midpoint(sorted[mid - 1], sorted[mid])),
        _ => Some(sorted[mid]),
    }
}

/// Percentile rank of `value` in `sample` with the mid-rank convention:
/// `(count below + 0.5 × count equal) / n × 100`. Tied values share one
/// rank, so the top of a tied group is not pushed down. `None` for an empty
/// sample or a non-finite value. Non-finite sample entries are ignored.
fn percentile_rank(sample: &[f64], value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let mut total = 0usize;
    let mut below = 0usize;
    let mut equal = 0usize;
    for candidate in sample.iter().filter(|v| v.is_finite()) {
        total += 1;
        match candidate.total_cmp(&value) {
            Ordering::Less => below += 1,
            Ordering::Equal => equal += 1,
            Ordering::Greater => {}
        }
    }
    if total == 0 {
        return None;
    }
    Some((below as f64 + 0.5 * equal as f64) / total as f64 * 100.0)
}

/// A competitive axis ranked against `sample_size` comparables.
#[allow(clippy::cast_possible_truncation)]
fn ranked_axis(
    axis: &str,
    value: f64,
    neighborhood_avg: Option<f64>,
    percentile: f64,
    sample_size: usize,
    assessment: &str,
) -> CompetitiveAxis {
    CompetitiveAxis {
        axis: axis.to_string(),
        listing_value: Some(value),
        neighborhood_avg,
        percentile: Some(percentile),
        sample_size: sample_size as u32,
        assessment: assessment.to_string(),
    }
}

/// A competitive axis that cannot be ranked. `assessment` says why.
fn unranked_axis(
    axis: &str,
    value: Option<f64>,
    neighborhood_avg: Option<f64>,
    assessment: &str,
) -> CompetitiveAxis {
    CompetitiveAxis {
        axis: axis.to_string(),
        listing_value: value,
        neighborhood_avg,
        percentile: None,
        sample_size: 0,
        assessment: assessment.to_string(),
    }
}

/// Three-level assessment of a 0-100 rank.
fn tier(percentile: f64, high: &'static str, mid: &'static str, low: &'static str) -> &'static str {
    if percentile >= 70.0 {
        high
    } else if percentile >= 40.0 {
        mid
    } else {
        low
    }
}

/// Keep only the prices quoted in the most common currency (the first one
/// seen wins a tie), so averages never mix currencies. `None` when `priced`
/// is empty.
fn prices_in_dominant_currency(priced: &[(&str, f64)]) -> Option<(String, Vec<f64>)> {
    let mut order: Vec<&str> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &(currency, _) in priced {
        let count = counts.entry(currency).or_insert(0);
        if *count == 0 {
            order.push(currency);
        }
        *count += 1;
    }
    let mut best: Option<(&str, usize)> = None;
    for currency in order {
        let count = counts.get(currency).copied().unwrap_or(0);
        if best.is_none_or(|(_, top)| count > top) {
            best = Some((currency, count));
        }
    }
    let (currency, _) = best?;
    let prices = priced
        .iter()
        .filter(|&&(c, price)| c == currency && price.is_finite() && price > 0.0)
        .map(|&(_, price)| price)
        .collect();
    Some((currency.to_string(), prices))
}

/// A calendar day's price when it is usable as data (finite and positive).
fn known_day_price(day: &CalendarDay) -> Option<f64> {
    day.price.filter(|p| p.is_finite() && *p > 0.0)
}

/// Friday and Saturday nights are the weekend nights.
fn is_weekend_night(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Fri | Weekday::Sat)
}

/// Mean weekend (Fri-Sat) and weekday prices of dated prices.
fn weekend_weekday_means(priced: &[(NaiveDate, f64)]) -> (Option<f64>, Option<f64>) {
    let weekend: Vec<f64> = priced
        .iter()
        .filter(|(date, _)| is_weekend_night(*date))
        .map(|(_, price)| *price)
        .collect();
    let weekday: Vec<f64> = priced
        .iter()
        .filter(|(date, _)| !is_weekend_night(*date))
        .map(|(_, price)| *price)
        .collect();
    (mean(&weekend), mean(&weekday))
}

/// Format an amount with the data's own currency. Never assumes `$`.
fn format_money(currency: Option<&str>, amount: f64, decimals: usize) -> String {
    match currency.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) if code.chars().all(|c| c.is_ascii_alphabetic()) => {
            format!("{code} {amount:.decimals$}")
        }
        Some(symbol) => format!("{symbol}{amount:.decimals$}"),
        None => format!("{amount:.decimals$} (currency unknown)"),
    }
}

/// Whole-unit amount with the data's own currency.
fn money(currency: Option<&str>, amount: f64) -> String {
    format_money(currency, amount, 0)
}

/// Like `money`, with cents.
fn money2(currency: Option<&str>, amount: f64) -> String {
    format_money(currency, amount, 2)
}

/// A trimmed, non-empty string, or `None`.
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// Listings of a search page that belong to the anchor's host.
///
/// Match by host id when both ids are known. Otherwise match by display
/// name, only when both names are present and the candidate has no
/// conflicting host id. A missing name never matches a missing name.
fn select_host_siblings<'a>(
    anchor: &ListingDetail,
    candidates: &'a [Listing],
) -> (HostMatch, Vec<&'a Listing>) {
    let anchor_id = non_empty(anchor.host_id.as_deref());
    if let Some(host_id) = anchor_id {
        let by_id: Vec<&Listing> = candidates
            .iter()
            .filter(|l| non_empty(l.host_id.as_deref()) == Some(host_id))
            .collect();
        if !by_id.is_empty() {
            return (HostMatch::HostId, by_id);
        }
    }
    if let Some(name) = non_empty(anchor.host_name.as_deref()) {
        let by_name: Vec<&Listing> = candidates
            .iter()
            .filter(|l| non_empty(l.host_name.as_deref()) == Some(name))
            .filter(|l| match (anchor_id, non_empty(l.host_id.as_deref())) {
                (Some(expected), Some(found)) => expected == found,
                _ => true,
            })
            .collect();
        if !by_name.is_empty() {
            return (HostMatch::HostName, by_name);
        }
    }
    (HostMatch::AnchorOnly, Vec::new())
}

/// Aggregate a search page into neighborhood statistics.
///
/// Price statistics use only listings with a known price in the most common
/// currency (`priced_listings` of `total_listings`). The superhost share is
/// the share of listings flagged superhost, and is `None` when no listing
/// reports the flag at all.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_neighborhood_stats(location: &str, listings: &[Listing]) -> NeighborhoodStats {
    let total_listings = listings.len() as u32;

    let priced: Vec<(&str, f64)> = listings
        .iter()
        .filter_map(|l| l.known_price().map(|price| (l.currency.as_str(), price)))
        .collect();
    let (currency, mut prices) = match prices_in_dominant_currency(&priced) {
        Some((currency, prices)) => (Some(currency), prices),
        None => (None, Vec::new()),
    };
    prices.sort_by(f64::total_cmp);

    let ratings: Vec<f64> = listings
        .iter()
        .filter_map(|l| l.rating)
        .filter(|r| r.is_finite())
        .collect();

    let mut type_counts: HashMap<String, u32> = HashMap::new();
    for listing in listings {
        let property_type = listing
            .property_type
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        *type_counts.entry(property_type).or_insert(0) += 1;
    }
    let mut property_type_distribution: Vec<PropertyTypeCount> = type_counts
        .into_iter()
        .map(|(property_type, count)| PropertyTypeCount {
            property_type,
            count,
            percentage: f64::from(count) / f64::from(total_listings) * 100.0,
        })
        .collect();
    property_type_distribution.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.property_type.cmp(&b.property_type))
    });

    let superhost_percentage = if listings.iter().any(|l| l.is_superhost.is_some()) {
        let flagged = listings
            .iter()
            .filter(|l| l.is_superhost == Some(true))
            .count();
        Some(flagged as f64 / f64::from(total_listings) * 100.0)
    } else {
        None
    };

    NeighborhoodStats {
        location: location.to_string(),
        total_listings,
        average_price: mean(&prices),
        median_price: median_sorted(&prices),
        price_range: match (prices.first(), prices.last()) {
            (Some(low), Some(high)) => Some((*low, *high)),
            _ => None,
        },
        average_rating: mean(&ratings),
        property_type_distribution,
        superhost_percentage,
        currency,
        priced_listings: prices.len() as u32,
    }
}

/// Estimate occupancy from a calendar. This is the single occupancy
/// definition shared by the occupancy, revenue and competitive tools (I7).
///
/// * Past days (`UnavailabilityReason::PastDate`) are excluded from the
///   numerator and the denominator, and counted in `past_days_excluded`.
/// * Host-blocked and stay-rule days are excluded the same way, and counted
///   in `blocked_days_excluded`.
/// * Every other unavailable night counts as occupied. Airbnb does not say
///   whether it is booked or blocked, so the rate is an upper bound.
/// * Days whose date does not parse are ignored.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_occupancy_estimate(listing_id: &str, calendar: &PriceCalendar) -> OccupancyEstimate {
    let mut past_days_excluded = 0u32;
    let mut blocked_days_excluded = 0u32;
    let mut considered: Vec<(&CalendarDay, NaiveDate, DayStatus)> = Vec::new();
    for day in &calendar.days {
        let Some(date) = day.parsed_date() else {
            continue;
        };
        match day.status() {
            DayStatus::Past => past_days_excluded += 1,
            DayStatus::Blocked => blocked_days_excluded += 1,
            status => considered.push((day, date, status)),
        }
    }

    let total_days = considered.len() as u32;
    let occupied_days = considered
        .iter()
        .filter(|(_, _, status)| *status == DayStatus::Occupied)
        .count() as u32;
    let available_days = total_days - occupied_days;
    let occupancy_rate = if total_days > 0 {
        f64::from(occupied_days) / f64::from(total_days) * 100.0
    } else {
        0.0
    };

    let open_prices: Vec<(NaiveDate, f64)> = considered
        .iter()
        .filter(|(_, _, status)| *status == DayStatus::Open)
        .filter_map(|(day, date, _)| known_day_price(day).map(|price| (*date, price)))
        .collect();
    let all_prices: Vec<f64> = open_prices.iter().map(|(_, price)| *price).collect();
    let average_available_price = mean(&all_prices);
    let (weekend_avg_price, weekday_avg_price) = weekend_weekday_means(&open_prices);

    let first_date = considered.iter().map(|(_, date, _)| *date).min();
    let last_date = considered.iter().map(|(_, date, _)| *date).max();

    let mut monthly: BTreeMap<String, (u32, u32, Vec<f64>)> = BTreeMap::new();
    for (day, date, status) in &considered {
        let entry = monthly
            .entry(date.format("%Y-%m").to_string())
            .or_insert((0, 0, Vec::new()));
        entry.0 += 1;
        if *status == DayStatus::Occupied {
            entry.1 += 1;
        } else if let Some(price) = known_day_price(day) {
            entry.2.push(price);
        }
    }
    let monthly_breakdown: Vec<MonthlyOccupancy> = monthly
        .into_iter()
        .map(|(month, (total, occupied, prices))| MonthlyOccupancy {
            month,
            total_days: total,
            occupied_days: occupied,
            available_days: total - occupied,
            occupancy_rate: f64::from(occupied) / f64::from(total) * 100.0,
            average_price: mean(&prices),
        })
        .collect();

    OccupancyEstimate {
        listing_id: listing_id.to_string(),
        period_start: first_date.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string()),
        period_end: last_date.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string()),
        total_days,
        occupied_days,
        available_days,
        occupancy_rate,
        past_days_excluded,
        blocked_days_excluded,
        currency: calendar.currency.clone(),
        average_available_price,
        weekend_avg_price,
        weekday_avg_price,
        monthly_breakdown,
    }
}

// ---------------------------------------------------------------------------
// Price Trends computation
// ---------------------------------------------------------------------------

/// Seasonal price analysis from the calendar's open, priced, future nights.
///
/// Past days are left out. When Airbnb publishes no price for any open night
/// (every calendar observed since 2026-09), every price field is `None` and
/// the result only describes availability.
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn compute_price_trends(listing_id: &str, calendar: &PriceCalendar) -> PriceTrends {
    let days: Vec<(&CalendarDay, NaiveDate)> = calendar
        .days
        .iter()
        .filter_map(|day| day.parsed_date().map(|date| (day, date)))
        .filter(|(day, _)| day.status() != DayStatus::Past)
        .collect();

    let priced: Vec<(NaiveDate, f64)> = days
        .iter()
        .filter(|(day, _)| day.status() == DayStatus::Open)
        .filter_map(|(day, date)| known_day_price(day).map(|price| (*date, price)))
        .collect();
    let prices: Vec<f64> = priced.iter().map(|(_, price)| *price).collect();

    let overall_avg = mean(&prices);
    let overall_min = prices.iter().copied().reduce(f64::min);
    let overall_max = prices.iter().copied().reduce(f64::max);
    let price_volatility = match overall_avg {
        Some(avg) if prices.len() > 1 => {
            let variance =
                prices.iter().map(|p| (p - avg).powi(2)).sum::<f64>() / prices.len() as f64;
            Some(variance.sqrt() / avg)
        }
        _ => None,
    };

    let (weekend_avg, weekday_avg) = weekend_weekday_means(&priced);
    let weekend_premium_pct = match (weekend_avg, weekday_avg) {
        (Some(weekend), Some(weekday)) => Some((weekend - weekday) / weekday * 100.0),
        _ => None,
    };

    let mut by_month: BTreeMap<String, Vec<(&CalendarDay, NaiveDate)>> = BTreeMap::new();
    for (day, date) in &days {
        by_month
            .entry(date.format("%Y-%m").to_string())
            .or_default()
            .push((*day, *date));
    }
    let monthly: Vec<MonthlyPriceSummary> = by_month
        .into_iter()
        .map(|(month, month_days)| {
            let open_days = month_days
                .iter()
                .filter(|(day, _)| day.status() == DayStatus::Open)
                .count();
            let month_priced: Vec<(NaiveDate, f64)> = month_days
                .iter()
                .filter(|(day, _)| day.status() == DayStatus::Open)
                .filter_map(|(day, date)| known_day_price(day).map(|price| (*date, price)))
                .collect();
            let month_prices: Vec<f64> = month_priced.iter().map(|(_, price)| *price).collect();
            let (weekend_avg, weekday_avg) = weekend_weekday_means(&month_priced);
            MonthlyPriceSummary {
                month,
                avg_price: mean(&month_prices),
                min_price: month_prices.iter().copied().reduce(f64::min),
                max_price: month_prices.iter().copied().reduce(f64::max),
                weekend_avg,
                weekday_avg,
                available_days: open_days as u32,
                total_days: month_days.len() as u32,
            }
        })
        .collect();

    let peak_month = monthly
        .iter()
        .filter_map(|m| m.avg_price.map(|avg| (avg, &m.month)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, month)| month.clone());
    let off_peak_month = monthly
        .iter()
        .filter_map(|m| m.avg_price.map(|avg| (avg, &m.month)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, month)| month.clone());

    let mut by_weekday: HashMap<Weekday, Vec<f64>> = HashMap::new();
    for (date, price) in &priced {
        by_weekday.entry(date.weekday()).or_default().push(*price);
    }
    let day_of_week: Vec<DayOfWeekPrice> = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ]
    .iter()
    .filter_map(|weekday| {
        by_weekday.get(weekday).and_then(|prices| {
            mean(prices).map(|avg| DayOfWeekPrice {
                day: weekday.to_string(),
                avg_price: avg,
                sample_count: prices.len() as u32,
            })
        })
    })
    .collect();

    let first_date = days.iter().map(|(_, date)| *date).min();
    let last_date = days.iter().map(|(_, date)| *date).max();

    PriceTrends {
        listing_id: listing_id.to_string(),
        currency: calendar.currency.clone(),
        period_start: first_date.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string()),
        period_end: last_date.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string()),
        overall_avg,
        overall_min,
        overall_max,
        price_volatility,
        weekend_premium_pct,
        peak_month,
        off_peak_month,
        monthly,
        day_of_week,
        priced_nights: prices.len() as u32,
    }
}

// ---------------------------------------------------------------------------
// Gap Finder computation
// ---------------------------------------------------------------------------

/// Longest open stretch still reported as a gap (the tool promises 1-3).
const MAX_GAP_NIGHTS: usize = 3;

/// Find 1-3 night openings between two occupied nights.
///
/// Only days inside one run of consecutive dates are adjacent nights
/// (`PriceCalendar::contiguous_runs`), so a missing date is an edge. Past and
/// host-blocked days are calendar edges too, not bookings, so an open
/// stretch next to them is not a gap. Longer stretches are unsold
/// inventory, not gaps. Each gap is compared with the minimum stay of its
/// first night, and the suggestion only ever lowers that minimum.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_gap_finder(listing_id: &str, calendar: &PriceCalendar) -> GapFinderResult {
    let mut gaps = Vec::new();

    for run in calendar.contiguous_runs() {
        let mut i = 0;
        while i < run.len() {
            if run[i].status() != DayStatus::Open {
                i += 1;
                continue;
            }
            let start = i;
            while i < run.len() && run[i].status() == DayStatus::Open {
                i += 1;
            }
            let bordered_before = start > 0 && run[start - 1].status() == DayStatus::Occupied;
            let bordered_after = i < run.len() && run[i].status() == DayStatus::Occupied;
            let open = &run[start..i];
            if !bordered_before || !bordered_after || open.len() > MAX_GAP_NIGHTS {
                continue;
            }
            let (Some(first), Some(last)) = (open.first(), open.last()) else {
                continue;
            };
            let nights = open.len() as u32;
            let prices: Vec<f64> = open.iter().filter_map(known_day_price).collect();
            let avg_price = mean(&prices);
            let min_nights = first.min_nights;
            gaps.push(CalendarGap {
                start_date: first.date.clone(),
                end_date: last.date.clone(),
                nights,
                avg_price,
                potential_revenue: avg_price.map(|avg| avg * f64::from(nights)),
                gap_type: String::from(if nights == 1 { "orphan" } else { "short_gap" }),
                min_nights,
                bookable_as_is: min_nights.map(|min| nights >= min),
            });
        }
    }

    let total_gaps = gaps.len() as u32;
    let total_gap_nights: u32 = gaps.iter().map(|g| g.nights).sum();
    let orphan_nights = gaps.iter().filter(|g| g.gap_type == "orphan").count() as u32;
    let short_gaps = gaps.iter().filter(|g| g.gap_type == "short_gap").count() as u32;
    let unbookable_gaps = gaps
        .iter()
        .filter(|g| g.bookable_as_is == Some(false))
        .count() as u32;
    let suggested_min_nights = gaps
        .iter()
        .filter(|g| g.bookable_as_is == Some(false))
        .map(|g| g.nights)
        .min();
    let revenues: Vec<f64> = gaps.iter().filter_map(|g| g.potential_revenue).collect();
    let potential_lost_revenue = if revenues.is_empty() {
        None
    } else {
        Some(revenues.iter().sum())
    };

    GapFinderResult {
        listing_id: listing_id.to_string(),
        currency: calendar.currency.clone(),
        total_gaps,
        total_gap_nights,
        orphan_nights,
        short_gaps,
        unbookable_gaps,
        potential_lost_revenue,
        gaps,
        suggested_min_nights,
    }
}

// ---------------------------------------------------------------------------
// Revenue Estimate computation
// ---------------------------------------------------------------------------

/// Project revenue from measured data where possible.
///
/// * ADR, in order of preference: the calendar's open-night prices, the
///   listing's own known price, then the neighborhood average (a proxy).
///   With none of them the result is `InsufficientData`, never `$0`.
/// * Occupancy comes from the calendar through `compute_occupancy_estimate`
///   (future nights only). Without a calendar (location-only estimates) it
///   is `ASSUMED_OCCUPANCY_PCT`, labelled as an assumption. A calendar with
///   no future night is `InsufficientData`.
/// * The currency is the one of the ADR's source, never a default `$`.
pub fn compute_revenue_estimate(
    listing_id: Option<&str>,
    location: &str,
    listing: Option<&ListingDetail>,
    calendar: Option<&PriceCalendar>,
    neighborhood: Option<&NeighborhoodStats>,
) -> crate::error::Result<RevenueEstimate> {
    let occupancy = calendar
        .map(|cal| compute_occupancy_estimate(listing_id.unwrap_or(cal.listing_id.as_str()), cal));

    let calendar_adr = occupancy
        .as_ref()
        .and_then(|o| o.average_available_price.map(|p| (p, o.currency.clone())));
    let listing_adr = listing.and_then(|d| d.known_price().map(|p| (p, d.currency.clone())));
    let neighborhood_adr = neighborhood.and_then(|n| {
        n.average_price
            .filter(|p| p.is_finite() && *p > 0.0)
            .map(|p| (p, n.currency.clone().unwrap_or_default()))
    });
    let (projected_adr, currency, adr_source) = if let Some((p, c)) = calendar_adr {
        (p, c, DataSource::Calendar)
    } else if let Some((p, c)) = listing_adr {
        (p, c, DataSource::ListingPrice)
    } else if let Some((p, c)) = neighborhood_adr {
        (p, c, DataSource::NeighborhoodAverage)
    } else {
        return Err(AirbnbError::InsufficientData {
            reason: "no nightly price is known: Airbnb's calendar publishes no prices, the listing has no dated price, and no priced comparable listing was found".into(),
        });
    };

    let (projected_occupancy_pct, occupancy_source, occupancy_nights_measured) = match occupancy
        .as_ref()
    {
        Some(o) => match o.measured_rate() {
            Some(rate) => (rate, DataSource::Calendar, o.total_days),
            None => {
                return Err(AirbnbError::InsufficientData {
                    reason: "the calendar window has no future night to measure occupancy".into(),
                });
            }
        },
        None => (ASSUMED_OCCUPANCY_PCT, DataSource::Assumption, 0),
    };

    let vs_neighborhood_avg_price_pct = if adr_source == DataSource::NeighborhoodAverage {
        None
    } else {
        neighborhood.and_then(|n| match (n.average_price, n.currency.as_deref()) {
            (Some(avg), Some(c)) if avg > 0.0 && c == currency => {
                Some((projected_adr - avg) / avg * 100.0)
            }
            _ => None,
        })
    };

    let monthly_breakdown: Vec<MonthlyRevenue> = occupancy.as_ref().map_or_else(Vec::new, |o| {
        o.monthly_breakdown
            .iter()
            .map(|m| {
                let rate = m.average_price.unwrap_or(projected_adr);
                MonthlyRevenue {
                    month: m.month.clone(),
                    projected_revenue: rate * f64::from(m.occupied_days),
                    projected_occupancy_pct: m.occupancy_rate,
                    avg_nightly_rate: rate,
                }
            })
            .collect()
    });

    let projected_monthly_revenue =
        projected_adr * (projected_occupancy_pct / 100.0) * NIGHTS_PER_MONTH;

    Ok(RevenueEstimate {
        listing_id: listing_id.map(str::to_string),
        location: location.to_string(),
        projected_adr,
        adr_source,
        projected_occupancy_pct,
        occupancy_source,
        occupancy_nights_measured,
        projected_monthly_revenue,
        projected_annual_revenue: projected_monthly_revenue * 12.0,
        vs_neighborhood_avg_price_pct,
        currency,
        monthly_breakdown,
    })
}

// ---------------------------------------------------------------------------
// Listing Score computation
// ---------------------------------------------------------------------------

/// Score a listing's quality (0-100) across photos, description, amenities,
/// reviews, host and pricing. Categories without data are `None` and left
/// out of the overall score.
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn compute_listing_score(
    detail: &ListingDetail,
    neighborhood: Option<&NeighborhoodStats>,
) -> ListingScore {
    let mut categories = Vec::new();
    let mut suggestions = Vec::new();

    // Photos score (0-100)
    let photo_count = detail.photos.len();
    let photo_score = match photo_count {
        0 => {
            suggestions.push(
                "Add photos to your listing — listings with 20+ photos perform best".to_string(),
            );
            0.0
        }
        1..=4 => {
            suggestions.push("Add more photos (aim for 20+)".to_string());
            25.0
        }
        5..=9 => {
            suggestions.push("Consider adding more photos (aim for 20+)".to_string());
            50.0
        }
        10..=19 => 75.0,
        _ => 100.0,
    };
    categories.push(CategoryScore {
        category: "Photos".to_string(),
        score: Some(photo_score),
        details: format!("{photo_count} photos"),
    });

    // Description score, counted in characters (not UTF-8 bytes)
    let desc_len = detail.description.chars().count();
    let desc_score = match desc_len {
        0 => {
            suggestions.push("Add a detailed description".to_string());
            0.0
        }
        1..=99 => {
            suggestions.push("Expand your description (aim for 500+ characters)".to_string());
            25.0
        }
        100..=299 => {
            suggestions.push("Consider adding more detail to your description".to_string());
            50.0
        }
        300..=499 => 75.0,
        _ => 100.0,
    };
    categories.push(CategoryScore {
        category: "Description".to_string(),
        score: Some(desc_score),
        details: format!("{desc_len} characters"),
    });

    // Amenities score
    let amenity_count = detail.amenities.len();
    let amenity_score = match amenity_count {
        0 => {
            suggestions.push(
                "List your amenities — this significantly impacts search ranking".to_string(),
            );
            0.0
        }
        1..=5 => {
            suggestions.push("Add more amenities (top listings have 20+)".to_string());
            25.0
        }
        6..=14 => {
            suggestions.push("Consider listing more amenities".to_string());
            50.0
        }
        15..=24 => 75.0,
        _ => 100.0,
    };
    categories.push(CategoryScore {
        category: "Amenities".to_string(),
        score: Some(amenity_score),
        details: format!("{amenity_count} amenities listed"),
    });

    // Reviews: volume tier blended with the guest rating
    let volume_score = match detail.review_count {
        0 => {
            suggestions.push("New listing — focus on getting your first reviews".to_string());
            0.0
        }
        1..=4 => 25.0,
        5..=19 => 50.0,
        20..=49 => 75.0,
        _ => 100.0,
    };
    let rating = detail.rating.filter(|r| r.is_finite());
    let reviews_score = match rating {
        Some(r) if detail.review_count > 0 => {
            if r < 4.5 {
                suggestions.push(format!(
                    "Guest rating {r:.2} is below 4.5 — address the most frequent complaints"
                ));
            }
            f64::midpoint(volume_score, rating_quality_score(r))
        }
        _ => volume_score,
    };
    let rating_info = rating.map_or_else(|| "no rating".to_string(), |r| format!("{r:.2} rating"));
    categories.push(CategoryScore {
        category: "Reviews".to_string(),
        score: Some(reviews_score),
        details: format!("{} reviews, {rating_info}", detail.review_count),
    });

    // Host: superhost, response rate (proportional) and response time
    let mut host_score: f64 = 50.0;
    let mut host_details: Vec<String> = Vec::new();
    if detail.host_is_superhost == Some(true) {
        host_score += 30.0;
        host_details.push("Superhost".to_string());
    }
    if let Some(rate) = detail.host_response_rate.as_deref().and_then(parse_percent) {
        host_score += 10.0 * rate / 100.0;
        host_details.push(format!("response rate {rate:.0}%"));
    }
    if let Some(time) = detail
        .host_response_time
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        host_score += if time.to_lowercase().contains("hour") {
            10.0
        } else {
            5.0
        };
        host_details.push(format!("response time: {time}"));
    }
    host_score = host_score.min(100.0);
    categories.push(CategoryScore {
        category: "Host".to_string(),
        score: Some(host_score),
        details: if host_details.is_empty() {
            "basic profile".to_string()
        } else {
            host_details.join(", ")
        },
    });

    // Pricing vs neighborhood average (known price, same currency only)
    let (pricing_score, pricing_details) = pricing_category(detail, neighborhood, &mut suggestions);
    categories.push(CategoryScore {
        category: "Pricing".to_string(),
        score: pricing_score,
        details: pricing_details,
    });

    let scored: Vec<f64> = categories.iter().filter_map(|c| c.score).collect();
    let overall_score = mean(&scored).unwrap_or(0.0);

    ListingScore {
        listing_id: detail.id.clone(),
        overall_score,
        category_scores: categories,
        suggestions,
    }
}

// ---------------------------------------------------------------------------
// Amenity Analysis computation
// ---------------------------------------------------------------------------

/// Normalize amenity names to canonical forms for consistent comparison.
fn normalize_amenity(name: &str) -> String {
    let lowered = name.trim().to_lowercase();
    match lowered.as_str() {
        "wi-fi" | "wi fi" | "wireless internet" | "wifi included" | "free wifi" => {
            "wifi".to_string()
        }
        "air conditioning" | "a/c" | "ac" | "central air" | "central air conditioning" => {
            "air conditioning".to_string()
        }
        "washer" | "washing machine" | "washer/dryer" => "washer".to_string(),
        "dryer" | "clothes dryer" => "dryer".to_string(),
        "tv" | "television" | "cable tv" | "hdtv" => "tv".to_string(),
        "hot tub" | "jacuzzi" | "spa" => "hot tub".to_string(),
        "bbq" | "bbq grill" | "barbecue" | "grill" => "bbq grill".to_string(),
        "parking" | "free parking" | "free parking on premises" | "off-street parking" => {
            "parking".to_string()
        }
        "pool" | "swimming pool" | "private pool" | "shared pool" => "pool".to_string(),
        "gym" | "fitness center" | "exercise equipment" => "gym".to_string(),
        "kitchen" | "full kitchen" | "kitchenette" => "kitchen".to_string(),
        "self check-in" | "self-check-in" | "keypad" | "lockbox" | "smart lock" => {
            "self check-in".to_string()
        }
        "heating" | "central heating" | "radiant heating" => "heating".to_string(),
        "iron" | "iron & board" | "iron and board" => "iron".to_string(),
        "hair dryer" | "hairdryer" | "blow dryer" => "hair dryer".to_string(),
        "essentials" | "towels" | "bed linens" | "bed sheets" => "essentials".to_string(),
        "smoke alarm" | "smoke detector" => "smoke alarm".to_string(),
        "carbon monoxide alarm" | "carbon monoxide detector" | "co detector" => {
            "carbon monoxide alarm".to_string()
        }
        "first aid kit" | "first-aid kit" => "first aid kit".to_string(),
        "fire extinguisher" | "fire blanket" => "fire extinguisher".to_string(),
        other => other.to_string(),
    }
}

/// Distinct normalized amenities of a listing.
fn normalized_amenities(detail: &ListingDetail) -> HashSet<String> {
    detail
        .amenities
        .iter()
        .map(|a| normalize_amenity(a))
        .filter(|a| !a.is_empty())
        .collect()
}

/// Compare a listing's amenities with comparable listings.
///
/// Each listing's amenities are normalized and de-duplicated first
/// (`Towels` and `Bed linens` are both `essentials`), so frequencies never
/// exceed 100% and identical listings score 100%. Comparables without any
/// amenity (typically parse failures) are ignored. With no usable
/// comparable, `amenity_score_pct` is `None` instead of a fake 100%.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_amenity_analysis(
    detail: &ListingDetail,
    neighborhood_details: &[ListingDetail],
) -> AmenityAnalysis {
    let listing_amenities = normalized_amenities(detail);
    let listing_amenity_count = listing_amenities.len() as u32;

    let neighbor_sets: Vec<HashSet<String>> = neighborhood_details
        .iter()
        .filter(|d| d.id != detail.id)
        .map(normalized_amenities)
        .filter(|set| !set.is_empty())
        .collect();
    let comparables_analyzed = neighbor_sets.len() as u32;
    let neighbor_amenity_counts: Vec<u32> =
        neighbor_sets.iter().map(|set| set.len() as u32).collect();
    let counts: Vec<f64> = neighbor_amenity_counts
        .iter()
        .map(|&count| f64::from(count))
        .collect();
    let neighborhood_avg_amenity_count = mean(&counts).unwrap_or(0.0);

    let mut amenity_freq: HashMap<&str, u32> = HashMap::new();
    for set in &neighbor_sets {
        for amenity in set {
            *amenity_freq.entry(amenity.as_str()).or_insert(0) += 1;
        }
    }

    let mut missing_popular = Vec::new();
    let mut present_rare = Vec::new();
    if comparables_analyzed > 0 {
        let comparables = f64::from(comparables_analyzed);
        for (&amenity, &count) in &amenity_freq {
            let freq_pct = f64::from(count) / comparables * 100.0;
            if !listing_amenities.contains(amenity) && freq_pct >= 50.0 {
                missing_popular.push(AmenityGap {
                    amenity: amenity.to_string(),
                    neighborhood_frequency_pct: freq_pct,
                    is_present: false,
                });
            }
        }
        for amenity in &listing_amenities {
            let count = amenity_freq.get(amenity.as_str()).copied().unwrap_or(0);
            let freq_pct = f64::from(count) / comparables * 100.0;
            if freq_pct < 30.0 {
                present_rare.push(AmenityGap {
                    amenity: amenity.clone(),
                    neighborhood_frequency_pct: freq_pct,
                    is_present: true,
                });
            }
        }
    }

    missing_popular.sort_by(|a, b| {
        b.neighborhood_frequency_pct
            .total_cmp(&a.neighborhood_frequency_pct)
            .then_with(|| a.amenity.cmp(&b.amenity))
    });
    present_rare.sort_by(|a, b| {
        a.neighborhood_frequency_pct
            .total_cmp(&b.neighborhood_frequency_pct)
            .then_with(|| a.amenity.cmp(&b.amenity))
    });

    let amenity_score_pct = if neighborhood_avg_amenity_count > 0.0 {
        Some((f64::from(listing_amenity_count) / neighborhood_avg_amenity_count * 100.0).min(200.0))
    } else {
        None
    };

    AmenityAnalysis {
        listing_id: detail.id.clone(),
        listing_amenity_count,
        neighborhood_avg_amenity_count,
        missing_popular_amenities: missing_popular,
        present_rare_amenities: present_rare,
        amenity_score_pct,
        comparables_analyzed,
        neighbor_amenity_counts,
    }
}

// ---------------------------------------------------------------------------
// Compare Listings computation
// ---------------------------------------------------------------------------

/// Compare listings side by side.
///
/// Price statistics and percentiles use only listings with a known price in
/// the most common currency. Percentiles follow the mid-rank convention, so
/// tied listings share one rank. When `details` holds the detail of a
/// listing (same id), its bedroom and listed-amenity counts are reported.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_compare_listings(
    listings: &[Listing],
    details: Option<&[ListingDetail]>,
) -> CompareListingsResult {
    let count = listings.len() as u32;
    let detail_of = |id: &str| details.and_then(|all| all.iter().find(|d| d.id == id));

    let priced: Vec<(&str, f64)> = listings
        .iter()
        .filter_map(|l| l.known_price().map(|price| (l.currency.as_str(), price)))
        .collect();
    let (currency, mut prices) = match prices_in_dominant_currency(&priced) {
        Some((currency, prices)) => (Some(currency), prices),
        None => (None, Vec::new()),
    };
    prices.sort_by(f64::total_cmp);

    let ratings: Vec<f64> = listings
        .iter()
        .filter_map(|l| l.rating)
        .filter(|r| r.is_finite())
        .collect();
    let superhost_count = listings
        .iter()
        .filter(|l| l.is_superhost == Some(true))
        .count() as u32;

    let comparisons: Vec<ListingComparison> = listings
        .iter()
        .map(|l| {
            let price_percentile = match (l.known_price(), currency.as_deref()) {
                (Some(price), Some(dominant)) if l.currency == dominant => {
                    percentile_rank(&prices, price)
                }
                _ => None,
            };
            let detail = detail_of(&l.id);
            ListingComparison {
                id: l.id.clone(),
                name: l.name.clone(),
                price_per_night: l.price_per_night,
                currency: l.currency.clone(),
                rating: l.rating,
                review_count: l.review_count,
                property_type: l.property_type.clone(),
                is_superhost: l.is_superhost,
                bedrooms: detail.and_then(|d| d.bedrooms),
                amenities_count: detail.map(|d| d.amenities.len() as u32),
                price_percentile,
                rating_percentile: l
                    .rating
                    .and_then(|rating| percentile_rank(&ratings, rating)),
            }
        })
        .collect();

    let priced_count = prices.len() as u32;
    CompareListingsResult {
        listings: comparisons,
        summary: ComparisonSummary {
            count,
            priced_count,
            currency,
            avg_price: mean(&prices),
            median_price: median_sorted(&prices),
            avg_rating: mean(&ratings),
            price_range: match (prices.first(), prices.last()) {
                (Some(low), Some(high)) => Some((*low, *high)),
                _ => None,
            },
            superhost_count,
        },
    }
}

// ---------------------------------------------------------------------------
// Market Comparison computation
// ---------------------------------------------------------------------------

pub fn compute_market_comparison(stats: &[NeighborhoodStats]) -> MarketComparison {
    let locations = stats
        .iter()
        .map(|s| MarketSnapshot {
            location: s.location.clone(),
            total_listings: s.total_listings,
            avg_price: s.average_price,
            median_price: s.median_price,
            avg_rating: s.average_rating,
            superhost_pct: s.superhost_percentage,
            top_property_type: s
                .property_type_distribution
                .first()
                .map(|pt| pt.property_type.clone()),
            currency: s.currency.clone(),
        })
        .collect();

    MarketComparison { locations }
}

// ---------------------------------------------------------------------------
// Host Portfolio computation
// ---------------------------------------------------------------------------

/// Build a host's portfolio from the listings of one search page.
///
/// Other properties are matched by host id, or by display name only when
/// both names are present and no conflicting host id is known (never
/// `None == None`). The queried listing is always included, exactly once.
/// Only the first search page for `search_location` is examined, and the
/// output says so.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_host_portfolio(
    anchor: &ListingDetail,
    search_location: &str,
    candidates: &[Listing],
) -> HostPortfolio {
    let (matched_by, siblings) = select_host_siblings(anchor, candidates);

    let mut properties = vec![PortfolioProperty {
        id: anchor.id.clone(),
        name: anchor.name.clone(),
        location: anchor.location.clone(),
        price_per_night: anchor.known_price(),
        currency: anchor.currency.clone(),
        rating: anchor.rating.filter(|r| r.is_finite()),
        review_count: anchor.review_count,
        property_type: anchor.property_type.clone(),
    }];
    let mut seen: HashSet<&str> = HashSet::from([anchor.id.as_str()]);
    for listing in siblings {
        if seen.insert(listing.id.as_str()) {
            properties.push(PortfolioProperty {
                id: listing.id.clone(),
                name: listing.name.clone(),
                location: listing.location.clone(),
                price_per_night: listing.known_price(),
                currency: listing.currency.clone(),
                rating: listing.rating.filter(|r| r.is_finite()),
                review_count: listing.review_count,
                property_type: listing.property_type.clone(),
            });
        }
    }

    let priced: Vec<(&str, f64)> = properties
        .iter()
        .filter_map(|p| p.price_per_night.map(|price| (p.currency.as_str(), price)))
        .collect();
    let (currency, mut prices) = match prices_in_dominant_currency(&priced) {
        Some((currency, prices)) => (Some(currency), prices),
        None => (None, Vec::new()),
    };
    prices.sort_by(f64::total_cmp);
    let ratings: Vec<f64> = properties.iter().filter_map(|p| p.rating).collect();
    let total_reviews: u32 = properties.iter().map(|p| p.review_count).sum();

    HostPortfolio {
        host_name: non_empty(anchor.host_name.as_deref())
            .map_or_else(|| "Unknown Host".to_string(), str::to_string),
        host_id: non_empty(anchor.host_id.as_deref()).map(str::to_string),
        total_properties: properties.len() as u32,
        avg_rating: mean(&ratings),
        avg_price: mean(&prices),
        price_range: match (prices.first(), prices.last()) {
            (Some(low), Some(high)) => Some((*low, *high)),
            _ => None,
        },
        currency,
        total_reviews,
        is_superhost: anchor.host_is_superhost,
        matched_by,
        search_location: search_location.to_string(),
        host_reported_listings: anchor.host_total_listings,
        properties,
    }
}

// ---------------------------------------------------------------------------
// Review Sentiment computation
// ---------------------------------------------------------------------------

/// Positive sentiment keywords (English, whole words).
const POSITIVE_KEYWORDS: &[&str] = &[
    "amazing",
    "beautiful",
    "perfect",
    "clean",
    "great",
    "lovely",
    "excellent",
    "wonderful",
    "comfortable",
    "spacious",
    "friendly",
    "helpful",
    "quiet",
    "cozy",
    "stunning",
];

/// Negative sentiment keywords (English, whole words).
const NEGATIVE_KEYWORDS: &[&str] = &[
    "dirty",
    "noisy",
    "broken",
    "disappointing",
    "uncomfortable",
    "small",
    "smelly",
    "rude",
    "cold",
    "late",
    "missing",
    "poor",
    "terrible",
    "awful",
    "worst",
];

/// Words that flip the polarity of a keyword following them within
/// `NEGATION_WINDOW` tokens of the same clause.
const NEGATORS: &[&str] = &[
    "not", "no", "never", "nothing", "hardly", "without", "nor", "isn't", "wasn't", "weren't",
    "aren't", "don't", "doesn't", "didn't", "won't", "can't", "couldn't",
];

/// How many tokens before a keyword a negator may appear.
const NEGATION_WINDOW: usize = 3;

/// Review themes and the words (or their plurals) that mention them.
const THEMES: &[(&str, &[&str])] = &[
    (
        "Cleanliness",
        &["clean", "dirty", "spotless", "dust", "tidy", "stain"],
    ),
    (
        "Location",
        &[
            "location",
            "area",
            "neighborhood",
            "walk",
            "transport",
            "central",
            "convenient",
            "nearby",
        ],
    ),
    (
        "Communication",
        &[
            "communication",
            "host",
            "responsive",
            "helpful",
            "friendly",
            "rude",
            "contact",
        ],
    ),
    (
        "Amenities",
        &[
            "amenities",
            "kitchen",
            "wifi",
            "pool",
            "bed",
            "bathroom",
            "towel",
            "equipment",
        ],
    ),
    (
        "Value",
        &[
            "value",
            "price",
            "expensive",
            "cheap",
            "worth",
            "overpriced",
            "bargain",
            "money",
        ],
    ),
];

/// Lower-cased word tokens of each clause. Clauses end at `. ! ? ; ,` or a
/// line break. Apostrophes stay inside words (`wasn't`).
fn clauses(text: &str) -> Vec<Vec<String>> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(['.', '!', '?', ';', ',', '\n'])
        .map(|clause| {
            clause
                .split(|c: char| !(c.is_alphanumeric() || c == '\''))
                .filter(|token| !token.is_empty())
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .filter(|tokens| !tokens.is_empty())
        .collect()
}

/// Count the sentiment hits of one clause, flipping the polarity of a
/// keyword preceded by a negator within `NEGATION_WINDOW` tokens. Each hit
/// is tallied, with negated keywords recorded as `not <keyword>`.
fn score_clause(
    tokens: &[String],
    positive_tally: &mut HashMap<String, u32>,
    negative_tally: &mut HashMap<String, u32>,
) -> (u32, u32) {
    let mut positive = 0u32;
    let mut negative = 0u32;
    for (index, token) in tokens.iter().enumerate() {
        let is_positive = POSITIVE_KEYWORDS.contains(&token.as_str());
        let is_negative = NEGATIVE_KEYWORDS.contains(&token.as_str());
        if !is_positive && !is_negative {
            continue;
        }
        let window_start = index.saturating_sub(NEGATION_WINDOW);
        let negated = tokens[window_start..index]
            .iter()
            .any(|t| NEGATORS.contains(&t.as_str()));
        let label = if negated {
            format!("not {token}")
        } else {
            token.clone()
        };
        if is_positive == negated {
            negative += 1;
            *negative_tally.entry(label).or_insert(0) += 1;
        } else {
            positive += 1;
            *positive_tally.entry(label).or_insert(0) += 1;
        }
    }
    (positive, negative)
}

/// Whether a clause mentions one of `keywords` (whole word or its plural).
fn mentions(tokens: &[String], keywords: &[&str]) -> bool {
    tokens.iter().any(|token| {
        keywords
            .iter()
            .any(|keyword| token.as_str() == *keyword || token.strip_suffix('s') == Some(*keyword))
    })
}

/// Reviews the English keyword lists can score: English or unknown
/// language, or text already translated.
fn is_english_text(review: &Review) -> bool {
    review.is_translated == Some(true)
        || review
            .language
            .as_deref()
            .is_none_or(|language| language.to_ascii_lowercase().starts_with("en"))
}

/// The 10 most frequent keywords, ties broken alphabetically.
fn top_keywords(tally: HashMap<String, u32>) -> Vec<(String, u32)> {
    let mut keywords: Vec<(String, u32)> = tally.into_iter().collect();
    keywords.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    keywords.truncate(10);
    keywords
}

/// Keyword-based review sentiment (heuristic).
///
/// Whole-word English keywords with simple negation (`not clean` counts as
/// negative, `never cold` as positive). Non-English reviews are skipped and
/// counted in `skipped_non_english`. A theme's polarity comes from the
/// clauses that mention it, not from the whole review.
#[allow(clippy::cast_possible_truncation)]
pub fn compute_review_sentiment(listing_id: &str, reviews: &[Review]) -> ReviewSentiment {
    let english: Vec<&Review> = reviews.iter().filter(|r| is_english_text(r)).collect();
    let skipped_non_english = (reviews.len() - english.len()) as u32;
    let total = english.len() as u32;

    let mut positive_count = 0u32;
    let mut negative_count = 0u32;
    let mut neutral_count = 0u32;
    let mut positive_tally: HashMap<String, u32> = HashMap::new();
    let mut negative_tally: HashMap<String, u32> = HashMap::new();
    // Per theme: (mentions, positive reviews, negative reviews, sample quotes)
    let mut theme_data: Vec<(u32, u32, u32, Vec<String>)> =
        vec![(0, 0, 0, Vec::new()); THEMES.len()];

    for review in &english {
        let mut review_positive = 0u32;
        let mut review_negative = 0u32;
        let mut theme_net: Vec<Option<i64>> = vec![None; THEMES.len()];
        for tokens in clauses(&review.comment) {
            let (positive, negative) =
                score_clause(&tokens, &mut positive_tally, &mut negative_tally);
            review_positive += positive;
            review_negative += negative;
            for (net, (_, keywords)) in theme_net.iter_mut().zip(THEMES) {
                if mentions(&tokens, keywords) {
                    *net = Some(net.unwrap_or(0) + i64::from(positive) - i64::from(negative));
                }
            }
        }
        match review_positive.cmp(&review_negative) {
            Ordering::Greater => positive_count += 1,
            Ordering::Less => negative_count += 1,
            Ordering::Equal => neutral_count += 1,
        }
        for (data, net) in theme_data.iter_mut().zip(&theme_net) {
            let Some(net) = *net else {
                continue;
            };
            data.0 += 1;
            match net.cmp(&0) {
                Ordering::Greater => data.1 += 1,
                Ordering::Less => data.2 += 1,
                Ordering::Equal => {}
            }
            if data.3.len() < 2 {
                data.3.push(review.comment.chars().take(100).collect());
            }
        }
    }

    let (positive_pct, negative_pct, neutral_pct) = if total > 0 {
        (
            f64::from(positive_count) / f64::from(total) * 100.0,
            f64::from(negative_count) / f64::from(total) * 100.0,
            f64::from(neutral_count) / f64::from(total) * 100.0,
        )
    } else {
        (0.0, 0.0, 0.0)
    };

    let mut themes: Vec<ReviewTheme> = THEMES
        .iter()
        .zip(theme_data)
        .filter(|(_, data)| data.0 > 0)
        .map(
            |((name, _), (mention_count, positive, negative, quotes))| ReviewTheme {
                theme: (*name).to_string(),
                mention_count,
                positive_count: positive,
                negative_count: negative,
                sample_quotes: quotes,
            },
        )
        .collect();
    themes.sort_by(|a, b| {
        b.mention_count
            .cmp(&a.mention_count)
            .then_with(|| a.theme.cmp(&b.theme))
    });

    ReviewSentiment {
        listing_id: listing_id.to_string(),
        total_reviews_analyzed: total,
        positive_pct,
        negative_pct,
        neutral_pct,
        themes,
        top_positive_keywords: top_keywords(positive_tally),
        top_negative_keywords: top_keywords(negative_tally),
        skipped_non_english,
    }
}

// ---------------------------------------------------------------------------
// Competitive Positioning computation
// ---------------------------------------------------------------------------

/// Rank a listing against the comparable listings of a location search on
/// price value, rating, amenity count and review volume, using mid-rank
/// percentile ranks and no fixed benchmarks. Occupancy is shown when
/// measured but not ranked, because no neighborhood occupancy benchmark
/// exists. Axes that cannot be ranked are left out of the overall score and
/// of strengths/weaknesses.
#[allow(clippy::too_many_lines)]
pub fn compute_competitive_positioning(
    detail: &ListingDetail,
    comparables: &[Listing],
    occupancy: Option<&OccupancyEstimate>,
    amenity_analysis: Option<&AmenityAnalysis>,
) -> CompetitivePositioning {
    let others: Vec<&Listing> = comparables.iter().filter(|l| l.id != detail.id).collect();
    let mut axes = Vec::with_capacity(5);

    // 1. Price value: cheaper than more comparables means better value.
    let prices: Vec<f64> = others
        .iter()
        .copied()
        .filter(|l| l.currency == detail.currency)
        .filter_map(Listing::known_price)
        .collect();
    axes.push(match detail.known_price() {
        None => unranked_axis(
            "Price Value",
            None,
            mean(&prices),
            "No known nightly price for this listing (Airbnb shows prices only for dated searches)",
        ),
        Some(price) => match percentile_rank(&prices, price) {
            Some(rank) => {
                let value = 100.0 - rank;
                ranked_axis(
                    "Price Value",
                    price,
                    mean(&prices),
                    value,
                    prices.len(),
                    tier(value, "Strong value", "Fair value", "Premium priced"),
                )
            }
            None => unranked_axis(
                "Price Value",
                Some(price),
                None,
                "No priced comparable listing in the same currency",
            ),
        },
    });

    // 2. Rating
    let ratings: Vec<f64> = others
        .iter()
        .filter_map(|l| l.rating)
        .filter(|r| r.is_finite())
        .collect();
    axes.push(match detail.rating.filter(|r| r.is_finite()) {
        None => unranked_axis("Rating", None, mean(&ratings), "Listing has no rating yet"),
        Some(rating) => match percentile_rank(&ratings, rating) {
            Some(rank) => ranked_axis(
                "Rating",
                rating,
                mean(&ratings),
                rank,
                ratings.len(),
                tier(rank, "Above average", "Average", "Below average"),
            ),
            None => unranked_axis("Rating", Some(rating), None, "No rated comparable listing"),
        },
    });

    // 3. Amenity count (distinct normalized amenities)
    axes.push(match amenity_analysis {
        Some(aa) if !aa.neighbor_amenity_counts.is_empty() => {
            let counts: Vec<f64> = aa
                .neighbor_amenity_counts
                .iter()
                .map(|&count| f64::from(count))
                .collect();
            let own = f64::from(aa.listing_amenity_count);
            match percentile_rank(&counts, own) {
                Some(rank) => ranked_axis(
                    "Amenity Count",
                    own,
                    Some(aa.neighborhood_avg_amenity_count),
                    rank,
                    counts.len(),
                    tier(rank, "Well equipped", "Adequate", "Under-equipped"),
                ),
                None => unranked_axis(
                    "Amenity Count",
                    Some(own),
                    None,
                    "No comparable listing with amenity data",
                ),
            }
        }
        _ => unranked_axis(
            "Amenity Count",
            Some(detail.amenities.len() as f64),
            None,
            "No comparable listing with amenity data",
        ),
    });

    // 4. Review volume
    let review_counts: Vec<f64> = others.iter().map(|l| f64::from(l.review_count)).collect();
    let own_reviews = f64::from(detail.review_count);
    axes.push(match percentile_rank(&review_counts, own_reviews) {
        Some(rank) => ranked_axis(
            "Review Volume",
            own_reviews,
            mean(&review_counts),
            rank,
            review_counts.len(),
            tier(rank, "Well reviewed", "Moderate reviews", "Few reviews"),
        ),
        None => unranked_axis(
            "Review Volume",
            Some(own_reviews),
            None,
            "No comparable listing",
        ),
    });

    // 5. Occupancy: measured for this listing only, never ranked.
    axes.push(match occupancy.and_then(OccupancyEstimate::measured_rate) {
        Some(rate) => unranked_axis(
            "Occupancy",
            Some(rate),
            None,
            "Measured share of future nights unavailable (booked or host-blocked); no neighborhood benchmark, so not ranked",
        ),
        None => unranked_axis("Occupancy", None, None, "No occupancy data"),
    });

    let ranked: Vec<f64> = axes.iter().filter_map(|a| a.percentile).collect();
    let overall_competitiveness = mean(&ranked);
    let strengths: Vec<String> = axes
        .iter()
        .filter(|a| a.percentile.is_some_and(|p| p >= 70.0))
        .map(|a| a.axis.clone())
        .collect();
    let weaknesses: Vec<String> = axes
        .iter()
        .filter(|a| a.percentile.is_some_and(|p| p <= 30.0))
        .map(|a| a.axis.clone())
        .collect();

    CompetitivePositioning {
        listing_id: detail.id.clone(),
        axes,
        overall_competitiveness,
        strengths,
        weaknesses,
    }
}

// ---------------------------------------------------------------------------
// Optimal Pricing computation
// ---------------------------------------------------------------------------

/// Largest rating adjustment, in percent of the baseline.
const MAX_RATING_ADJUSTMENT_PCT: f64 = 20.0;
/// Largest amenity adjustment, in percent of the baseline.
const MAX_AMENITY_ADJUSTMENT_PCT: f64 = 10.0;

/// Recommend a nightly price.
///
/// Baseline: the neighborhood median, else the listing's own known price.
/// With neither, the result is `InsufficientData` instead of a `$1.00`
/// recommendation. Rating (±20%) and amenity (±10%) adjustments are capped.
/// The weekday/weekend split keeps both the measured weekend premium and a
/// weekly mean (5 weekday + 2 weekend nights) equal to the recommendation.
#[allow(clippy::too_many_lines)]
pub fn compute_optimal_pricing(
    detail: &ListingDetail,
    neighborhood: Option<&NeighborhoodStats>,
    price_trends: Option<&PriceTrends>,
    amenity_analysis: Option<&AmenityAnalysis>,
) -> crate::error::Result<PricingRecommendation> {
    let current_price = detail.known_price();
    let mut reasoning = Vec::new();

    let median = neighborhood.and_then(|n| {
        n.median_price
            .filter(|m| m.is_finite() && *m > 0.0)
            .map(|m| {
                (
                    m,
                    n.currency
                        .clone()
                        .unwrap_or_else(|| detail.currency.clone()),
                )
            })
    });
    let (baseline, currency) = if let Some((m, c)) = median.clone() {
        reasoning.push(format!(
            "Baseline: neighborhood median {}/night",
            money2(Some(c.as_str()), m)
        ));
        (m, c)
    } else if let Some(price) = current_price {
        reasoning.push(format!(
            "Baseline: current listing price {}/night (no neighborhood median)",
            money2(Some(detail.currency.as_str()), price)
        ));
        (price, detail.currency.clone())
    } else {
        return Err(AirbnbError::InsufficientData {
            reason: "no neighborhood median price and no known nightly price for this listing"
                .into(),
        });
    };

    let mut recommended = baseline;

    if let Some(rating) = detail.rating.filter(|r| r.is_finite())
        && let Some(avg_rating) = neighborhood
            .and_then(|n| n.average_rating)
            .filter(|a| a.is_finite() && *a > 0.0)
    {
        // Each 0.1 rating point above average is about +2%, capped.
        let pct = ((rating - avg_rating) * 20.0)
            .clamp(-MAX_RATING_ADJUSTMENT_PCT, MAX_RATING_ADJUSTMENT_PCT);
        recommended += baseline * pct / 100.0;
        reasoning.push(format!(
            "Rating adjustment: {pct:+.1}% (your {rating:.2} vs avg {avg_rating:.2})"
        ));
    }

    let amenity_premium_pct = match amenity_analysis.map(|aa| aa.amenity_score_pct) {
        Some(Some(score)) if score.is_finite() => {
            let pct = ((score - 100.0) * 0.1)
                .clamp(-MAX_AMENITY_ADJUSTMENT_PCT, MAX_AMENITY_ADJUSTMENT_PCT);
            if pct.abs() > f64::EPSILON {
                recommended += baseline * pct / 100.0;
                reasoning.push(format!(
                    "Amenity adjustment: {pct:+.1}% (amenity score {score:.0}% of the comparable average)"
                ));
            } else {
                reasoning.push("Amenities in line with comparable listings".to_string());
            }
            Some(pct)
        }
        Some(_) => {
            reasoning.push(
                "Amenity adjustment skipped: no comparable listing with amenity data".to_string(),
            );
            None
        }
        None => None,
    };

    let range_low = recommended * 0.85;
    let range_high = recommended * 1.15;

    let (weekday_recommendation, weekend_recommendation) = match price_trends
        .and_then(|t| t.weekend_premium_pct)
        .filter(|p| p.is_finite() && *p > -100.0)
    {
        Some(premium_pct) => {
            let premium = premium_pct / 100.0;
            let weekday = recommended * 7.0 / (5.0 + 2.0 * (1.0 + premium));
            let weekend = weekday * (1.0 + premium);
            reasoning.push(format!(
                "Weekend premium: {premium_pct:+.1}% measured in the calendar, applied so the weekly average stays at the recommended price"
            ));
            (Some(weekday), Some(weekend))
        }
        None => (None, None),
    };

    if current_price.is_some()
        && !detail.currency.is_empty()
        && !currency.is_empty()
        && detail.currency != currency
    {
        reasoning.push(format!(
            "Current price is in {} but the recommendation is in {currency}: not converted, so the difference is not comparable",
            detail.currency
        ));
    }

    if current_price.is_none() {
        reasoning.push(
            "Current price unknown: Airbnb shows nightly prices only for dated searches"
                .to_string(),
        );
    }

    Ok(PricingRecommendation {
        listing_id: detail.id.clone(),
        current_price,
        current_price_currency: detail.currency.clone(),
        recommended_price: recommended,
        recommended_range: (range_low, range_high),
        currency,
        reasoning,
        weekday_recommendation,
        weekend_recommendation,
        amenity_premium_pct,
        vs_neighborhood_median: median.map(|(m, _)| (recommended - m) / m * 100.0),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::calendar::UnavailabilityReason;
    use crate::test_helpers::{
        make_calendar_day, make_listing, make_listing_detail, make_price_calendar,
    };

    #[test]
    fn occupancy_and_trends_do_not_panic_on_non_ascii_dates() {
        // Byte 7 of both strings falls inside a multi-byte character.
        let days = vec![
            make_calendar_day("1 июня 2025", Some(100.0), false),
            make_calendar_day("1月15日", Some(100.0), true),
            make_calendar_day("2025-06-01", Some(120.0), true),
        ];
        let cal = make_price_calendar("42", days);

        let occupancy = compute_occupancy_estimate("42", &cal);
        let months: Vec<&str> = occupancy
            .monthly_breakdown
            .iter()
            .map(|m| m.month.as_str())
            .collect();
        assert_eq!(months, vec!["2025-06"]);

        let trends = compute_price_trends("42", &cal);
        let months: Vec<&str> = trends.monthly.iter().map(|m| m.month.as_str()).collect();
        assert_eq!(months, vec!["2025-06"]);
    }

    #[test]
    fn nan_ratings_are_ignored_by_aggregates() {
        let mut listings: Vec<Listing> = (0..30)
            .map(|i| make_listing(&i.to_string(), "L", 100.0 + f64::from(i)))
            .collect();
        listings[3].rating = Some(f64::NAN);
        listings[7].price_per_night = f64::NAN;

        let stats = compute_neighborhood_stats("Paris", &listings);
        assert!(stats.average_rating.is_some_and(f64::is_finite));
        assert!(stats.average_price.is_some_and(f64::is_finite));

        let compared = compute_compare_listings(&listings, None);
        assert!(compared.summary.avg_rating.is_some_and(f64::is_finite));
    }

    #[test]
    fn neighborhood_stats_basic() {
        let listings = vec![
            make_listing("1", "Apt A", 100.0),
            make_listing("2", "Apt B", 200.0),
            make_listing("3", "Apt C", 150.0),
        ];
        let stats = compute_neighborhood_stats("Paris", &listings);
        assert_eq!(stats.total_listings, 3);
        assert!((stats.average_price.unwrap() - 150.0).abs() < 0.01);
        assert!((stats.median_price.unwrap() - 150.0).abs() < 0.01);
        assert_eq!(stats.price_range.unwrap(), (100.0, 200.0));
    }

    #[test]
    fn neighborhood_stats_empty() {
        let stats = compute_neighborhood_stats("Nowhere", &[]);
        assert_eq!(stats.total_listings, 0);
        assert!(stats.average_price.is_none());
        assert!(stats.median_price.is_none());
        assert!(stats.price_range.is_none());
        assert!(stats.average_rating.is_none());
        assert!(stats.superhost_percentage.is_none());
    }

    #[test]
    fn neighborhood_stats_median_even() {
        let listings = vec![make_listing("1", "A", 100.0), make_listing("2", "B", 200.0)];
        let stats = compute_neighborhood_stats("Test", &listings);
        assert!((stats.median_price.unwrap() - 150.0).abs() < 0.01);
    }

    #[test]
    fn neighborhood_stats_ratings() {
        let listings = vec![
            make_listing("1", "A", 100.0), // rating = Some(4.5) from factory
            make_listing("2", "B", 200.0), // rating = Some(4.5) from factory
        ];
        let stats = compute_neighborhood_stats("Test", &listings);
        assert!((stats.average_rating.unwrap() - 4.5).abs() < 0.01);
    }

    #[test]
    fn neighborhood_stats_property_types() {
        let mut l1 = make_listing("1", "A", 100.0);
        l1.property_type = Some("Apartment".to_string());
        let mut l2 = make_listing("2", "B", 200.0);
        l2.property_type = Some("House".to_string());
        let mut l3 = make_listing("3", "C", 150.0);
        l3.property_type = Some("Apartment".to_string());

        let stats = compute_neighborhood_stats("Test", &[l1, l2, l3]);
        assert_eq!(stats.property_type_distribution.len(), 2);
        // Sorted by count desc: Apartment(2), House(1)
        assert_eq!(
            stats.property_type_distribution[0].property_type,
            "Apartment"
        );
        assert_eq!(stats.property_type_distribution[0].count, 2);
        assert!((stats.property_type_distribution[0].percentage - 66.666).abs() < 1.0);
    }

    #[test]
    fn neighborhood_stats_superhost_pct() {
        let mut l1 = make_listing("1", "A", 100.0);
        l1.is_superhost = Some(true);
        let l2 = make_listing("2", "B", 200.0); // is_superhost = None

        let stats = compute_neighborhood_stats("Test", &[l1, l2]);
        assert!((stats.superhost_percentage.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn neighborhood_stats_display() {
        let listings = vec![make_listing("1", "A", 100.0)];
        let stats = compute_neighborhood_stats("Paris", &listings);
        let s = stats.to_string();
        assert!(s.contains("Neighborhood: Paris"));
        assert!(s.contains("Listings analyzed: 1"));
    }

    #[test]
    fn occupancy_basic() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-06-02", Some(120.0), false),
            make_calendar_day("2025-06-03", Some(110.0), true),
            make_calendar_day("2025-06-04", Some(130.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);

        assert_eq!(est.total_days, 4);
        assert_eq!(est.occupied_days, 2);
        assert_eq!(est.available_days, 2);
        assert!((est.occupancy_rate - 50.0).abs() < 0.01);
        // Avg of available: (100 + 110) / 2 = 105
        assert!((est.average_available_price.unwrap() - 105.0).abs() < 0.01);
    }

    #[test]
    fn occupancy_empty_calendar() {
        let cal = make_price_calendar("42", vec![]);
        let est = compute_occupancy_estimate("42", &cal);

        assert_eq!(est.total_days, 0);
        assert_eq!(est.occupied_days, 0);
        assert!((est.occupancy_rate - 0.0).abs() < 0.01);
        assert!(est.average_available_price.is_none());
        assert!(est.weekend_avg_price.is_none());
        assert!(est.weekday_avg_price.is_none());
        assert!(est.monthly_breakdown.is_empty());
    }

    #[test]
    fn occupancy_weekend_weekday_split() {
        // 2025-06-06 = Friday, 2025-06-07 = Saturday, 2025-06-09 = Monday
        let days = vec![
            make_calendar_day("2025-06-06", Some(200.0), true), // Fri (weekend)
            make_calendar_day("2025-06-07", Some(250.0), true), // Sat (weekend)
            make_calendar_day("2025-06-09", Some(100.0), true), // Mon (weekday)
            make_calendar_day("2025-06-10", Some(110.0), true), // Tue (weekday)
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);

        // Weekend avg: (200+250)/2 = 225
        assert!((est.weekend_avg_price.unwrap() - 225.0).abs() < 0.01);
        // Weekday avg: (100+110)/2 = 105
        assert!((est.weekday_avg_price.unwrap() - 105.0).abs() < 0.01);
    }

    #[test]
    fn occupancy_monthly_breakdown() {
        let days = vec![
            make_calendar_day("2025-06-28", Some(100.0), true),
            make_calendar_day("2025-06-29", Some(100.0), false),
            make_calendar_day("2025-06-30", Some(100.0), true),
            make_calendar_day("2025-07-01", Some(150.0), true),
            make_calendar_day("2025-07-02", Some(150.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);

        assert_eq!(est.monthly_breakdown.len(), 2);
        assert_eq!(est.monthly_breakdown[0].month, "2025-06");
        assert_eq!(est.monthly_breakdown[0].total_days, 3);
        assert_eq!(est.monthly_breakdown[0].occupied_days, 1);
        assert_eq!(est.monthly_breakdown[1].month, "2025-07");
        assert_eq!(est.monthly_breakdown[1].total_days, 2);
        assert_eq!(est.monthly_breakdown[1].occupied_days, 1);
    }

    #[test]
    fn occupancy_period_boundaries() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-08-31", Some(200.0), true),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);

        assert_eq!(est.period_start, "2025-06-01");
        assert_eq!(est.period_end, "2025-08-31");
    }

    #[test]
    fn occupancy_no_prices() {
        let days = vec![
            make_calendar_day("2025-06-01", None, true),
            make_calendar_day("2025-06-02", None, false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);

        assert_eq!(est.total_days, 2);
        assert!(est.average_available_price.is_none());
        assert!(est.weekend_avg_price.is_none());
        assert!(est.weekday_avg_price.is_none());
    }

    #[test]
    fn occupancy_display() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-06-02", Some(120.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);
        let s = est.to_string();
        assert!(s.contains("listing 42"));
        assert!(s.contains("50.0%"));
    }

    #[test]
    fn neighborhood_stats_single_listing() {
        let listings = vec![make_listing("1", "Solo", 150.0)];
        let stats = compute_neighborhood_stats("Test", &listings);
        assert_eq!(stats.total_listings, 1);
        assert!((stats.median_price.unwrap() - 150.0).abs() < 0.01);
        assert!((stats.average_price.unwrap() - 150.0).abs() < 0.01);
        assert_eq!(stats.price_range, Some((150.0, 150.0)));
    }

    #[test]
    fn neighborhood_stats_all_none_ratings() {
        let mut l1 = make_listing("1", "A", 100.0);
        l1.rating = None;
        let mut l2 = make_listing("2", "B", 200.0);
        l2.rating = None;
        let stats = compute_neighborhood_stats("Test", &[l1, l2]);
        assert!(stats.average_rating.is_none());
    }

    #[test]
    fn neighborhood_stats_none_property_types() {
        let mut l1 = make_listing("1", "A", 100.0);
        l1.property_type = None;
        let stats = compute_neighborhood_stats("Test", &[l1]);
        assert_eq!(stats.property_type_distribution.len(), 1);
        assert_eq!(stats.property_type_distribution[0].property_type, "Unknown");
    }

    #[test]
    fn occupancy_all_occupied() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), false),
            make_calendar_day("2025-06-02", Some(120.0), false),
            make_calendar_day("2025-06-03", Some(110.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);
        assert!((est.occupancy_rate - 100.0).abs() < 0.01);
        assert_eq!(est.occupied_days, 3);
        assert_eq!(est.available_days, 0);
        assert!(est.average_available_price.is_none());
    }

    #[test]
    fn occupancy_all_available() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-06-02", Some(120.0), true),
            make_calendar_day("2025-06-03", Some(110.0), true),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_occupancy_estimate("42", &cal);
        assert!((est.occupancy_rate - 0.0).abs() < 0.01);
        assert_eq!(est.occupied_days, 0);
        assert_eq!(est.available_days, 3);
        assert!((est.average_available_price.unwrap() - 110.0).abs() < 0.01);
    }

    #[test]
    fn host_profile_display() {
        let profile = HostProfile {
            host_id: Some("123".to_string()),
            name: "Alice".to_string(),
            is_superhost: Some(true),
            response_rate: Some("98%".to_string()),
            response_time: Some("within an hour".to_string()),
            member_since: Some("2015".to_string()),
            languages: vec!["English".to_string(), "French".to_string()],
            total_listings: Some(5),
            description: Some("Experienced host".to_string()),
            profile_picture_url: None,
            identity_verified: Some(true),
        };
        let s = profile.to_string();
        assert!(s.contains("Host: Alice"));
        assert!(s.contains("Superhost: Yes"));
        assert!(s.contains("Response rate: 98%"));
        assert!(s.contains("English, French"));
        assert!(s.contains("Identity verified: Yes"));
    }

    #[test]
    fn host_profile_display_minimal() {
        let profile = HostProfile {
            host_id: None,
            name: "Bob".to_string(),
            is_superhost: None,
            response_rate: None,
            response_time: None,
            member_since: None,
            languages: vec![],
            total_listings: None,
            description: None,
            profile_picture_url: None,
            identity_verified: None,
        };
        let s = profile.to_string();
        assert!(s.contains("Host: Bob"));
        assert!(!s.contains("Superhost"));
        assert!(!s.contains("Response"));
    }

    // -----------------------------------------------------------------------
    // Price Trends tests
    // -----------------------------------------------------------------------

    #[test]
    fn price_trends_basic() {
        // 2025-06-06=Fri, 07=Sat, 09=Mon, 10=Tue
        let days = vec![
            make_calendar_day("2025-06-06", Some(200.0), true),
            make_calendar_day("2025-06-07", Some(250.0), true),
            make_calendar_day("2025-06-09", Some(100.0), true),
            make_calendar_day("2025-06-10", Some(110.0), true),
        ];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);

        assert_eq!(trends.listing_id, "42");
        assert!((trends.overall_avg.unwrap() - 165.0).abs() < 0.01);
        assert!((trends.overall_min.unwrap() - 100.0).abs() < 0.01);
        assert!((trends.overall_max.unwrap() - 250.0).abs() < 0.01);
        assert!(trends.price_volatility.unwrap() > 0.0);
        assert!(trends.weekend_premium_pct.is_some());
        // Weekend avg (200+250)/2=225, weekday avg (100+110)/2=105
        // Premium = (225-105)/105*100 = 114.3%
        assert!((trends.weekend_premium_pct.unwrap() - 114.28).abs() < 1.0);
    }

    #[test]
    fn price_trends_empty_calendar() {
        let cal = make_price_calendar("42", vec![]);
        let trends = compute_price_trends("42", &cal);

        assert!(trends.overall_avg.is_none());
        assert!(trends.weekend_premium_pct.is_none());
        assert!(trends.monthly.is_empty());
        assert!(trends.day_of_week.is_empty());
        assert!(trends.peak_month.is_none());
    }

    #[test]
    fn price_trends_monthly_breakdown() {
        let days = vec![
            make_calendar_day("2025-06-15", Some(100.0), true),
            make_calendar_day("2025-06-16", Some(120.0), true),
            make_calendar_day("2025-07-15", Some(200.0), true),
            make_calendar_day("2025-07-16", Some(220.0), true),
        ];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);

        assert_eq!(trends.monthly.len(), 2);
        assert_eq!(trends.monthly[0].month, "2025-06");
        assert!((trends.monthly[0].avg_price.unwrap() - 110.0).abs() < 0.01);
        assert_eq!(trends.monthly[1].month, "2025-07");
        assert!((trends.monthly[1].avg_price.unwrap() - 210.0).abs() < 0.01);
        assert_eq!(trends.peak_month.as_deref(), Some("2025-07"));
        assert_eq!(trends.off_peak_month.as_deref(), Some("2025-06"));
    }

    #[test]
    fn price_trends_display() {
        let days = vec![make_calendar_day("2025-06-15", Some(100.0), true)];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);
        let s = trends.to_string();
        assert!(s.contains("Price Trends: listing 42"));
    }

    // -----------------------------------------------------------------------
    // Gap Finder tests
    // -----------------------------------------------------------------------

    #[test]
    fn gap_finder_no_gaps_all_available() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-06-02", Some(100.0), true),
            make_calendar_day("2025-06-03", Some(100.0), true),
        ];
        let cal = make_price_calendar("42", days);
        let result = compute_gap_finder("42", &cal);
        assert_eq!(result.total_gaps, 0);
        assert_eq!(result.total_gap_nights, 0);
    }

    #[test]
    fn gap_finder_no_gaps_all_occupied() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), false),
            make_calendar_day("2025-06-02", Some(100.0), false),
            make_calendar_day("2025-06-03", Some(100.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let result = compute_gap_finder("42", &cal);
        assert_eq!(result.total_gaps, 0);
    }

    #[test]
    fn gap_finder_orphan_night() {
        // occupied - available - occupied
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), false),
            make_calendar_day("2025-06-02", Some(150.0), true),
            make_calendar_day("2025-06-03", Some(100.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let result = compute_gap_finder("42", &cal);

        assert_eq!(result.total_gaps, 1);
        assert_eq!(result.orphan_nights, 1);
        assert_eq!(result.gaps[0].gap_type, "orphan");
        assert_eq!(result.gaps[0].nights, 1);
        assert!((result.gaps[0].potential_revenue.unwrap() - 150.0).abs() < 0.01);
        // min_nights is 2 (factory default): the 1-night gap cannot sell as-is,
        // so the advice is to lower the minimum to 1, never to raise it.
        assert_eq!(result.unbookable_gaps, 1);
        assert_eq!(result.suggested_min_nights, Some(1));
    }

    #[test]
    fn gap_finder_does_not_bridge_missing_dates() {
        // 2026-10-03 is missing: the open night 10-02 touches a hole, not a booking.
        let days = vec![
            make_calendar_day("2026-10-01", Some(100.0), false),
            make_calendar_day("2026-10-02", Some(100.0), true),
            make_calendar_day("2026-10-04", Some(100.0), false),
        ];
        let result = compute_gap_finder("42", &make_price_calendar("42", days));
        assert_eq!(result.total_gaps, 0, "{:?}", result.gaps);
    }

    #[test]
    fn gap_finder_ignores_past_edges_and_long_vacancies() {
        // MATH-4 scenario: one booking Oct 10-12, window ends Nov 15, today Sep 28.
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from("2026-09-28", 12, true, None));
        days.extend(days_from(
            "2026-10-10",
            3,
            false,
            Some(UnavailabilityReason::Booked),
        ));
        days.extend(days_from("2026-10-13", 33, true, None));
        days.extend(days_from(
            "2026-11-15",
            16,
            false,
            Some(UnavailabilityReason::Unknown),
        ));
        for day in &mut days {
            day.price = Some(150.0);
        }
        let result = compute_gap_finder("42", &make_price_calendar("42", days));

        assert_eq!(result.total_gaps, 0);
        assert_eq!(result.total_gap_nights, 0);
        assert!(result.potential_lost_revenue.is_none());
    }

    #[test]
    fn gap_finder_ignores_short_stretches_next_to_past_or_blocked_days() {
        let gaps_in = |days: Vec<CalendarDay>| {
            compute_gap_finder("42", &make_price_calendar("42", days)).total_gaps
        };
        let booked = || Some(UnavailabilityReason::Booked);

        // (a) A 2-night open stretch that starts right after the elapsed days.
        let mut past_edge = days_from("2026-09-27", 1, false, Some(UnavailabilityReason::PastDate));
        past_edge.extend(days_from("2026-09-28", 2, true, None));
        past_edge.extend(days_from("2026-09-30", 1, false, booked()));
        assert_eq!(gaps_in(past_edge), 0);

        // (b) A 1-night open stretch that ends at a host-blocked day.
        let mut blocked_edge = days_from("2026-10-01", 1, false, booked());
        blocked_edge.extend(days_from("2026-10-02", 1, true, None));
        blocked_edge.extend(days_from(
            "2026-10-03",
            1,
            false,
            Some(UnavailabilityReason::BlockedByHost),
        ));
        assert_eq!(gaps_in(blocked_edge), 0);

        // (c) Control: the same stretch between two bookings is a gap.
        let mut control = days_from("2026-10-01", 1, false, booked());
        control.extend(days_from("2026-10-02", 1, true, None));
        control.extend(days_from("2026-10-03", 1, false, booked()));
        assert_eq!(gaps_in(control), 1);
    }

    #[test]
    fn gap_finder_never_suggests_raising_min_stay_for_orphans() {
        let pattern = [false, true, false, true, false, true, false];
        let days: Vec<CalendarDay> = pattern
            .iter()
            .enumerate()
            .map(|(i, &open)| {
                let mut day =
                    make_calendar_day(&format!("2026-10-{:02}", i + 1), Some(100.0), open);
                day.min_nights = Some(1);
                day
            })
            .collect();

        let result = compute_gap_finder("42", &make_price_calendar("42", days.clone()));
        assert_eq!(result.orphan_nights, 3);
        assert_eq!(result.unbookable_gaps, 0);
        assert_eq!(result.suggested_min_nights, None);

        let strict: Vec<CalendarDay> = days
            .into_iter()
            .map(|mut day| {
                day.min_nights = Some(2);
                day
            })
            .collect();
        let result = compute_gap_finder("42", &make_price_calendar("42", strict));
        assert_eq!(result.unbookable_gaps, 3);
        assert_eq!(result.suggested_min_nights, Some(1));
        assert_eq!(result.gaps[0].bookable_as_is, Some(false));
    }

    #[test]
    fn gap_finder_without_prices_reports_unknown_revenue() {
        let days = vec![
            make_calendar_day("2026-10-01", None, false),
            make_calendar_day("2026-10-02", None, true),
            make_calendar_day("2026-10-03", None, false),
        ];
        let result = compute_gap_finder("42", &make_price_calendar("42", days));
        assert_eq!(result.total_gaps, 1);
        assert!(result.potential_lost_revenue.is_none());
        let s = result.to_string();
        assert!(s.contains("unknown"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn gap_finder_display_uses_calendar_currency() {
        let mut cal = make_price_calendar(
            "42",
            vec![
                make_calendar_day("2026-10-01", Some(100.0), false),
                make_calendar_day("2026-10-02", Some(150.0), true),
                make_calendar_day("2026-10-03", Some(100.0), false),
            ],
        );
        cal.currency = "€".into();
        let s = compute_gap_finder("42", &cal).to_string();
        assert!(s.contains("€150"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn gap_finder_short_gap() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), false),
            make_calendar_day("2025-06-02", Some(150.0), true),
            make_calendar_day("2025-06-03", Some(150.0), true),
            make_calendar_day("2025-06-04", Some(100.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let result = compute_gap_finder("42", &cal);

        assert_eq!(result.total_gaps, 1);
        assert_eq!(result.short_gaps, 1);
        assert_eq!(result.gaps[0].gap_type, "short_gap");
        assert_eq!(result.gaps[0].nights, 2);
    }

    #[test]
    fn gap_finder_display() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), false),
            make_calendar_day("2025-06-02", Some(150.0), true),
            make_calendar_day("2025-06-03", Some(100.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let result = compute_gap_finder("42", &cal);
        let s = result.to_string();
        assert!(s.contains("Gap Analysis: listing 42"));
        assert!(s.contains("orphan"));
    }

    // -----------------------------------------------------------------------
    // Revenue Estimate tests
    // -----------------------------------------------------------------------

    #[test]
    fn revenue_estimate_with_calendar() {
        let days = vec![
            make_calendar_day("2025-06-01", Some(100.0), true),
            make_calendar_day("2025-06-02", Some(200.0), true),
            make_calendar_day("2025-06-03", Some(150.0), false),
        ];
        let cal = make_price_calendar("42", days);
        let est = compute_revenue_estimate(Some("42"), "Paris", None, Some(&cal), None).unwrap();

        assert_eq!(est.listing_id.as_deref(), Some("42"));
        assert!((est.projected_adr - 150.0).abs() < 0.01);
        assert!(est.projected_monthly_revenue > 0.0);
        assert!(est.projected_annual_revenue > 0.0);
    }

    #[test]
    fn revenue_estimate_neighborhood_only() {
        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(120.0),
            median_price: Some(110.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };
        let est = compute_revenue_estimate(None, "Paris", None, None, Some(&stats)).unwrap();

        assert!((est.projected_adr - 120.0).abs() < 0.01);
        assert!((est.projected_occupancy_pct - 65.0).abs() < 0.01); // industry default
    }

    #[test]
    fn revenue_estimate_display() {
        let est = RevenueEstimate {
            listing_id: Some("42".to_string()),
            location: "Paris".to_string(),
            projected_adr: 150.0,
            adr_source: DataSource::Calendar,
            projected_occupancy_pct: 70.0,
            occupancy_source: DataSource::Calendar,
            occupancy_nights_measured: 30,
            projected_monthly_revenue: 3150.0,
            projected_annual_revenue: 37800.0,
            vs_neighborhood_avg_price_pct: Some(25.0),
            currency: "$".to_string(),
            monthly_breakdown: vec![],
        };
        let s = est.to_string();
        assert!(s.contains("Revenue Estimate"));
        assert!(s.contains("$150"));
    }

    // -----------------------------------------------------------------------
    // Listing Score tests
    // -----------------------------------------------------------------------

    #[test]
    fn listing_score_basic() {
        let detail = make_listing_detail("42");
        let score = compute_listing_score(&detail, None);

        assert_eq!(score.listing_id, "42");
        assert!(score.overall_score > 0.0);
        assert!(!score.category_scores.is_empty());
    }

    #[test]
    fn listing_score_with_neighborhood() {
        let detail = make_listing_detail("42");
        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0), // same as detail price
            median_price: Some(95.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };
        let score = compute_listing_score(&detail, Some(&stats));

        let pricing_cat = score
            .category_scores
            .iter()
            .find(|c| c.category == "Pricing")
            .unwrap();
        assert!((pricing_cat.score.unwrap() - 100.0).abs() < 0.01); // price matches market
    }

    #[test]
    fn listing_score_display() {
        let detail = make_listing_detail("42");
        let score = compute_listing_score(&detail, None);
        let s = score.to_string();
        assert!(s.contains("Listing Score: 42"));
        assert!(s.contains("/100"));
    }

    // -----------------------------------------------------------------------
    // Amenity Analysis tests
    // -----------------------------------------------------------------------

    #[test]
    fn amenity_analysis_basic() {
        let mut detail = make_listing_detail("42");
        detail.amenities = vec!["WiFi".to_string(), "Pool".to_string()];

        let mut neighbor1 = make_listing_detail("1");
        neighbor1.amenities = vec!["WiFi".to_string(), "Kitchen".to_string(), "AC".to_string()];
        let mut neighbor2 = make_listing_detail("2");
        neighbor2.amenities = vec![
            "WiFi".to_string(),
            "Kitchen".to_string(),
            "Pool".to_string(),
        ];

        let analysis = compute_amenity_analysis(&detail, &[neighbor1, neighbor2]);

        assert_eq!(analysis.listing_amenity_count, 2);
        assert!((analysis.neighborhood_avg_amenity_count - 3.0).abs() < 0.01);
        // Kitchen is in 100% of neighbors, missing from listing (normalized to "kitchen")
        assert!(
            analysis
                .missing_popular_amenities
                .iter()
                .any(|a| a.amenity == "kitchen")
        );
    }

    #[test]
    fn amenity_analysis_empty_neighborhood() {
        let detail = make_listing_detail("42");
        let analysis = compute_amenity_analysis(&detail, &[]);

        assert_eq!(analysis.listing_amenity_count, 2); // WiFi, Kitchen from factory
        assert!((analysis.neighborhood_avg_amenity_count - 0.0).abs() < 0.01);
        assert!(analysis.missing_popular_amenities.is_empty());
    }

    #[test]
    fn amenity_analysis_display() {
        let detail = make_listing_detail("42");
        let analysis = compute_amenity_analysis(&detail, &[]);
        let s = analysis.to_string();
        assert!(s.contains("Amenity Analysis: listing 42"));
    }

    // -----------------------------------------------------------------------
    // Compare Listings tests
    // -----------------------------------------------------------------------

    #[test]
    fn compare_listings_basic() {
        let listings = vec![
            make_listing("1", "Cheap", 50.0),
            make_listing("2", "Mid", 100.0),
            make_listing("3", "Expensive", 200.0),
        ];
        let result = compute_compare_listings(&listings, None);

        assert_eq!(result.summary.count, 3);
        assert!((result.summary.avg_price.unwrap() - 116.66).abs() < 1.0);
        assert!((result.summary.median_price.unwrap() - 100.0).abs() < 0.01);
        assert_eq!(result.summary.price_range, Some((50.0, 200.0)));
        assert_eq!(result.listings.len(), 3);
    }

    #[test]
    fn compare_listings_single() {
        let listings = vec![make_listing("1", "Solo", 100.0)];
        let result = compute_compare_listings(&listings, None);
        assert_eq!(result.summary.count, 1);
        assert!((result.listings[0].price_percentile.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn compare_percentiles_share_rank_for_ties() {
        let listings: Vec<Listing> = [100.0, 150.0, 150.0, 150.0, 150.0]
            .iter()
            .enumerate()
            .map(|(i, &price)| make_listing(&i.to_string(), "L", price))
            .collect();
        let result = compute_compare_listings(&listings, None);
        let pct: Vec<f64> = result
            .listings
            .iter()
            .map(|l| l.price_percentile.unwrap())
            .collect();
        assert!((pct[0] - 10.0).abs() < 1e-9, "{pct:?}");
        for p in &pct[1..] {
            assert!((p - 60.0).abs() < 1e-9, "{pct:?}");
        }
    }

    #[test]
    fn compare_rating_percentile_ties_at_top_are_not_pushed_down() {
        let listings: Vec<Listing> = [4.8, 5.0, 5.0, 5.0, 5.0]
            .iter()
            .enumerate()
            .map(|(i, &rating)| {
                let mut l = make_listing(&i.to_string(), "L", 100.0);
                l.rating = Some(rating);
                l
            })
            .collect();
        let result = compute_compare_listings(&listings, None);
        assert!((result.listings[0].rating_percentile.unwrap() - 10.0).abs() < 1e-9);
        assert!((result.listings[1].rating_percentile.unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn compare_unknown_price_has_no_percentile_and_is_excluded() {
        let listings = vec![
            make_listing("1", "Unpriced", 0.0),
            make_listing("2", "B", 100.0),
            make_listing("3", "C", 200.0),
        ];
        let result = compute_compare_listings(&listings, None);
        assert!(result.listings[0].price_percentile.is_none());
        assert_eq!(result.summary.priced_count, 2);
        assert!((result.summary.avg_price.unwrap() - 150.0).abs() < 1e-9);
        assert_eq!(result.summary.price_range, Some((100.0, 200.0)));
        let s = result.to_string();
        let unpriced_row = s.lines().find(|l| l.contains("Unpriced")).unwrap();
        assert!(unpriced_row.contains("n/a"), "{unpriced_row}");
    }

    #[test]
    fn compare_summary_uses_listing_currency() {
        let listings: Vec<Listing> = [100.0, 200.0]
            .iter()
            .enumerate()
            .map(|(i, &price)| {
                let mut l = make_listing(&i.to_string(), "L", price);
                l.currency = "€".into();
                l
            })
            .collect();
        let s = compute_compare_listings(&listings, None).to_string();
        assert!(s.contains("avg €150/night"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn compare_uses_details_for_bedrooms_and_amenities() {
        // Q-6 (handed over by P1b): `details` was ignored and both fields
        // were hard-coded to None.
        let listings = vec![make_listing("1", "A", 100.0), make_listing("2", "B", 200.0)];
        let details = vec![make_listing_detail("1")]; // 2 bedrooms, 2 amenities
        let result = compute_compare_listings(&listings, Some(&details));
        assert_eq!(result.listings[0].bedrooms, Some(2));
        assert_eq!(result.listings[0].amenities_count, Some(2));
        assert_eq!(result.listings[1].bedrooms, None);
        assert_eq!(result.listings[1].amenities_count, None);
    }

    #[test]
    fn compare_listings_empty() {
        let result = compute_compare_listings(&[], None);
        assert_eq!(result.summary.count, 0);
        assert!(result.listings.is_empty());
    }

    #[test]
    fn compare_listings_display() {
        let listings = vec![make_listing("1", "A", 100.0), make_listing("2", "B", 200.0)];
        let result = compute_compare_listings(&listings, None);
        let s = result.to_string();
        assert!(s.contains("Listing Comparison (2 listings)"));
    }

    // -----------------------------------------------------------------------
    // Market Comparison tests
    // -----------------------------------------------------------------------

    #[test]
    fn market_comparison_basic() {
        let stats = vec![
            compute_neighborhood_stats(
                "Paris",
                &[make_listing("1", "A", 150.0), make_listing("2", "B", 200.0)],
            ),
            compute_neighborhood_stats("London", &[make_listing("3", "C", 250.0)]),
        ];
        let result = compute_market_comparison(&stats);
        assert_eq!(result.locations.len(), 2);
        assert_eq!(result.locations[0].location, "Paris");
        assert_eq!(result.locations[0].total_listings, 2);
        assert_eq!(result.locations[1].location, "London");
    }

    #[test]
    fn market_comparison_display() {
        let stats = vec![compute_neighborhood_stats(
            "Paris",
            &[make_listing("1", "A", 100.0)],
        )];
        let result = compute_market_comparison(&stats);
        let s = result.to_string();
        assert!(s.contains("Market Comparison"));
        assert!(s.contains("Paris"));
    }

    // -----------------------------------------------------------------------
    // Host Portfolio tests
    // -----------------------------------------------------------------------

    fn portfolio_candidate(
        id: &str,
        host_name: Option<&str>,
        host_id: Option<&str>,
        price: f64,
    ) -> Listing {
        let mut listing = make_listing(id, &format!("Listing {id}"), price);
        listing.host_name = host_name.map(str::to_string);
        listing.host_id = host_id.map(str::to_string);
        listing
    }

    #[test]
    fn host_portfolio_unknown_host_does_not_claim_the_search_page() {
        let mut anchor = make_listing_detail("42");
        anchor.host_name = None;
        anchor.host_id = None;
        let candidates: Vec<Listing> = (1..=18)
            .map(|i| portfolio_candidate(&i.to_string(), None, None, 100.0))
            .collect();
        let portfolio = compute_host_portfolio(&anchor, "Paris", &candidates);

        assert_eq!(portfolio.total_properties, 1);
        assert_eq!(portfolio.matched_by, HostMatch::AnchorOnly);
        assert_eq!(portfolio.properties[0].id, "42");
        assert_eq!(portfolio.host_name, "Unknown Host");
    }

    #[test]
    fn host_portfolio_keeps_the_queried_listing() {
        let mut anchor = make_listing_detail("100");
        anchor.host_id = Some("h1".into());
        anchor.price_per_night = 250.0;
        anchor.rating = Some(4.95);
        anchor.review_count = 300;
        let mut sibling = portfolio_candidate("200", Some("Test Host"), Some("h1"), 90.0);
        sibling.rating = Some(4.6);
        sibling.review_count = 12;
        let other = portfolio_candidate("300", Some("Paul"), Some("h2"), 80.0);
        let portfolio = compute_host_portfolio(&anchor, "Paris", &[sibling, other]);

        assert_eq!(portfolio.matched_by, HostMatch::HostId);
        let ids: Vec<&str> = portfolio.properties.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["100", "200"]);
        assert_eq!(portfolio.total_reviews, 312);
        assert!((portfolio.avg_price.unwrap() - 170.0).abs() < 1e-9);
    }

    #[test]
    fn host_portfolio_counts_the_anchor_once_when_it_is_on_the_page() {
        let mut anchor = make_listing_detail("42");
        anchor.host_id = Some("h1".into());
        let candidates = vec![
            portfolio_candidate("42", Some("Test Host"), Some("h1"), 100.0),
            portfolio_candidate("43", Some("Test Host"), Some("h1"), 120.0),
        ];
        let portfolio = compute_host_portfolio(&anchor, "Paris", &candidates);
        let ids: Vec<&str> = portfolio.properties.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["42", "43"]);
    }

    #[test]
    fn host_portfolio_name_match_skips_other_hosts_with_the_same_name() {
        let mut anchor = make_listing_detail("42");
        anchor.host_name = Some("Marie".into());
        anchor.host_id = Some("h1".into()); // not on the page
        let candidates = vec![
            portfolio_candidate("5", Some("Marie"), Some("h2"), 100.0),
            portfolio_candidate("6", Some("Marie"), None, 110.0),
        ];
        let portfolio = compute_host_portfolio(&anchor, "Paris", &candidates);

        assert_eq!(portfolio.matched_by, HostMatch::HostName);
        let ids: Vec<&str> = portfolio.properties.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["42", "6"]);
        assert!(portfolio.to_string().contains("display name only"));
    }

    #[test]
    fn host_portfolio_display_states_coverage_and_unknown_prices() {
        let mut anchor = make_listing_detail("42");
        anchor.price_per_night = 0.0;
        anchor.host_total_listings = Some(3);
        let portfolio = compute_host_portfolio(&anchor, "Paris", &[]);
        let s = portfolio.to_string();

        assert!(portfolio.avg_price.is_none());
        assert!(s.contains("not the host's full portfolio"), "{s}");
        assert!(s.contains("Airbnb reports 3 listing(s)"), "{s}");
        assert!(s.contains("price unavailable"), "{s}");
        assert!(!s.contains("$0"), "{s}");
    }

    // -----------------------------------------------------------------------
    // Amenity normalization tests
    // -----------------------------------------------------------------------

    #[test]
    fn amenity_normalization_wifi_variants() {
        assert_eq!(normalize_amenity("Wi-Fi"), "wifi");
        assert_eq!(normalize_amenity("WiFi"), "wifi");
        assert_eq!(normalize_amenity("FREE WIFI"), "wifi");
        assert_eq!(normalize_amenity("Wireless Internet"), "wifi");
        assert_eq!(normalize_amenity("  wifi included  "), "wifi");
    }

    #[test]
    fn amenity_normalization_ac_variants() {
        assert_eq!(normalize_amenity("Air Conditioning"), "air conditioning");
        assert_eq!(normalize_amenity("A/C"), "air conditioning");
        assert_eq!(normalize_amenity("AC"), "air conditioning");
        assert_eq!(normalize_amenity("Central Air"), "air conditioning");
    }

    #[test]
    fn amenity_normalization_misc_variants() {
        assert_eq!(normalize_amenity("Swimming Pool"), "pool");
        assert_eq!(normalize_amenity("Private Pool"), "pool");
        assert_eq!(normalize_amenity("BBQ"), "bbq grill");
        assert_eq!(normalize_amenity("Barbecue"), "bbq grill");
        assert_eq!(normalize_amenity("Fitness Center"), "gym");
        assert_eq!(normalize_amenity("Full Kitchen"), "kitchen");
        assert_eq!(normalize_amenity("Kitchenette"), "kitchen");
        assert_eq!(normalize_amenity("HairDryer"), "hair dryer");
        assert_eq!(normalize_amenity("Smoke Detector"), "smoke alarm");
    }

    #[test]
    fn amenity_normalization_passthrough() {
        assert_eq!(normalize_amenity("Balcony"), "balcony");
        assert_eq!(normalize_amenity("  Garden View  "), "garden view");
    }

    #[test]
    fn amenity_analysis_with_normalization() {
        let mut detail = make_listing_detail("42");
        detail.amenities = vec!["Wi-Fi".into(), "A/C".into()];

        let mut neighbor = make_listing_detail("1");
        neighbor.amenities = vec![
            "WiFi".into(),
            "Air Conditioning".into(),
            "Swimming Pool".into(),
        ];

        let analysis = compute_amenity_analysis(&detail, &[neighbor]);

        // wifi and air conditioning should match via normalization — NOT missing
        assert!(
            !analysis
                .missing_popular_amenities
                .iter()
                .any(|a| a.amenity == "wifi")
        );
        assert!(
            !analysis
                .missing_popular_amenities
                .iter()
                .any(|a| a.amenity == "air conditioning")
        );
        // pool should be missing (100% of neighbors have it)
        assert!(
            analysis
                .missing_popular_amenities
                .iter()
                .any(|a| a.amenity == "pool")
        );
    }

    // -----------------------------------------------------------------------
    // Review Sentiment tests
    // -----------------------------------------------------------------------

    fn make_review(comment: &str) -> Review {
        Review {
            author: "TestUser".to_string(),
            date: "2025-01-01".to_string(),
            rating: Some(4.0),
            comment: comment.to_string(),
            response: None,
            reviewer_location: None,
            language: None,
            is_translated: None,
        }
    }

    #[test]
    fn test_review_sentiment_basic() {
        let reviews = vec![
            make_review("Amazing place, beautiful and clean!"),
            make_review("Terrible stay, dirty and noisy room"),
            make_review("Great location, wonderful host, very friendly"),
        ];
        let sentiment = compute_review_sentiment("42", &reviews);

        assert_eq!(sentiment.listing_id, "42");
        assert_eq!(sentiment.total_reviews_analyzed, 3);
        // 2 positive (review 1 and 3), 1 negative (review 2)
        assert!((sentiment.positive_pct - 66.66).abs() < 1.0);
        assert!((sentiment.negative_pct - 33.33).abs() < 1.0);
        assert!((sentiment.neutral_pct - 0.0).abs() < 0.01);
        assert!(!sentiment.top_positive_keywords.is_empty());
        assert!(!sentiment.top_negative_keywords.is_empty());
        // Themes should include Cleanliness (clean, dirty) and Communication (host, friendly)
        assert!(sentiment.themes.iter().any(|t| t.theme == "Cleanliness"));
    }

    #[test]
    fn review_sentiment_orders_keywords_and_themes_by_count_desc() {
        let reviews = vec![
            make_review("Clean, clean and clean again. Great host."),
            make_review("Very clean and great location."),
            make_review("Great view but dirty bathroom and dirty sheets, noisy street."),
        ];
        let sentiment = compute_review_sentiment("7", &reviews);

        let positive: Vec<u32> = sentiment
            .top_positive_keywords
            .iter()
            .map(|(_, count)| *count)
            .collect();
        assert!(
            positive.len() >= 2,
            "fixture must yield at least two positive keywords: {:?}",
            sentiment.top_positive_keywords
        );
        assert!(
            positive.is_sorted_by(|a, b| a >= b),
            "positive keywords must be sorted by count, descending: {:?}",
            sentiment.top_positive_keywords
        );

        let negative: Vec<u32> = sentiment
            .top_negative_keywords
            .iter()
            .map(|(_, count)| *count)
            .collect();
        assert!(
            negative.len() >= 2,
            "fixture must yield at least two negative keywords: {:?}",
            sentiment.top_negative_keywords
        );
        assert!(
            negative.is_sorted_by(|a, b| a >= b),
            "negative keywords must be sorted by count, descending: {:?}",
            sentiment.top_negative_keywords
        );

        let themes: Vec<u32> = sentiment
            .themes
            .iter()
            .map(|theme| theme.mention_count)
            .collect();
        assert!(
            themes.is_sorted_by(|a, b| a >= b),
            "themes must be sorted by mention count, descending: {themes:?}"
        );
    }

    #[test]
    fn test_review_sentiment_empty() {
        let sentiment = compute_review_sentiment("42", &[]);

        assert_eq!(sentiment.total_reviews_analyzed, 0);
        assert!((sentiment.positive_pct - 0.0).abs() < 0.01);
        assert!((sentiment.negative_pct - 0.0).abs() < 0.01);
        assert!((sentiment.neutral_pct - 0.0).abs() < 0.01);
        assert!(sentiment.themes.is_empty());
        assert!(sentiment.top_positive_keywords.is_empty());
        assert!(sentiment.top_negative_keywords.is_empty());
    }

    // -----------------------------------------------------------------------
    // Optimal Pricing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_optimal_pricing_basic() {
        let detail = make_listing_detail("42"); // price 100, rating 4.8
        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(95.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };
        let days = vec![
            make_calendar_day("2025-06-06", Some(200.0), true), // Fri
            make_calendar_day("2025-06-07", Some(250.0), true), // Sat
            make_calendar_day("2025-06-09", Some(100.0), true), // Mon
            make_calendar_day("2025-06-10", Some(110.0), true), // Tue
        ];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);

        let rec = compute_optimal_pricing(&detail, Some(&stats), Some(&trends), None).unwrap();

        assert_eq!(rec.listing_id, "42");
        assert!(rec.recommended_price > 0.0);
        assert!(rec.recommended_range.0 < rec.recommended_price);
        assert!(rec.recommended_range.1 > rec.recommended_price);
        assert!(!rec.reasoning.is_empty());
        // With a higher-than-average rating, recommended should be above median
        assert!(rec.recommended_price > 95.0);
        // Weekend / weekday should be present since we have trends
        assert!(rec.weekday_recommendation.is_some());
        assert!(rec.weekend_recommendation.is_some());
    }

    // -----------------------------------------------------------------------
    // Compare Listings deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn compare_listings_two_listings() {
        let mut l1 = make_listing("1", "Budget Apt", 80.0);
        l1.rating = Some(4.2);
        l1.is_superhost = Some(true);
        let mut l2 = make_listing("2", "Luxury Suite", 250.0);
        l2.rating = Some(4.9);

        let result = compute_compare_listings(&[l1, l2], None);

        assert_eq!(result.summary.count, 2);
        assert_eq!(result.listings.len(), 2);
        assert_eq!(result.summary.superhost_count, 1);
        assert_eq!(result.summary.price_range, Some((80.0, 250.0)));
        assert!(result.summary.avg_rating.is_some());

        // Verify both listings appear by name
        let names: Vec<&str> = result.listings.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"Budget Apt"));
        assert!(names.contains(&"Luxury Suite"));

        // Verify percentile ordering: cheaper listing should have lower price percentile
        let budget = result.listings.iter().find(|l| l.id == "1").unwrap();
        let luxury = result.listings.iter().find(|l| l.id == "2").unwrap();
        assert!(budget.price_percentile.unwrap() < luxury.price_percentile.unwrap());

        // Verify rating percentiles exist
        assert!(budget.rating_percentile.is_some());
        assert!(luxury.rating_percentile.is_some());
    }

    // -----------------------------------------------------------------------
    // Listing Score deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn listing_score_perfect() {
        let mut detail = make_listing_detail("42");
        // Fill in all fields to maximize score
        detail.photos = (0..25).map(|i| format!("photo_{i}.jpg")).collect();
        detail.description = "A".repeat(600); // >500 chars
        detail.amenities = (0..30).map(|i| format!("amenity_{i}")).collect();
        detail.review_count = 100;
        detail.rating = Some(4.95);
        detail.host_is_superhost = Some(true);
        detail.host_response_rate = Some("100%".to_string());
        detail.host_response_time = Some("within an hour".to_string());

        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(100.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };

        let score = compute_listing_score(&detail, Some(&stats));
        assert!(
            score.overall_score > 80.0,
            "Perfect listing should score > 80, got {}",
            score.overall_score
        );
        // Check each category is high
        for cat in &score.category_scores {
            let score = cat
                .score
                .expect("every category is scored for a complete listing");
            assert!(
                score >= 75.0,
                "Category {} should score >= 75, got {score}",
                cat.category
            );
        }
    }

    #[test]
    fn listing_score_minimal() {
        let mut detail = make_listing_detail("42");
        detail.photos = vec![];
        detail.description = String::new();
        detail.amenities = vec![];
        detail.review_count = 0;
        detail.rating = None;
        detail.host_is_superhost = None;
        detail.host_response_rate = None;
        detail.host_response_time = None;

        let score = compute_listing_score(&detail, None);
        // Minimal listing: photos=0, desc=0, amenities=0, reviews=0, host=50, pricing=50
        // Average should be low
        assert!(
            score.overall_score < 30.0,
            "Minimal listing should score < 30, got {}",
            score.overall_score
        );
        assert!(
            !score.suggestions.is_empty(),
            "Minimal listing should have suggestions"
        );
    }

    // -----------------------------------------------------------------------
    // Market Comparison deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn market_comparison_two_locations() {
        let paris_stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 50,
            average_price: Some(150.0),
            median_price: Some(140.0),
            price_range: Some((50.0, 500.0)),
            average_rating: Some(4.6),
            property_type_distribution: vec![PropertyTypeCount {
                property_type: "Apartment".to_string(),
                count: 40,
                percentage: 80.0,
            }],
            superhost_percentage: Some(35.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };
        let london_stats = NeighborhoodStats {
            location: "London".to_string(),
            total_listings: 80,
            average_price: Some(200.0),
            median_price: Some(180.0),
            price_range: Some((70.0, 800.0)),
            average_rating: Some(4.4),
            property_type_distribution: vec![PropertyTypeCount {
                property_type: "Flat".to_string(),
                count: 60,
                percentage: 75.0,
            }],
            superhost_percentage: Some(25.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };

        let result = compute_market_comparison(&[paris_stats, london_stats]);

        assert_eq!(result.locations.len(), 2);

        let paris = &result.locations[0];
        assert_eq!(paris.location, "Paris");
        assert_eq!(paris.total_listings, 50);
        assert!((paris.avg_price.unwrap() - 150.0).abs() < 0.01);
        assert!((paris.median_price.unwrap() - 140.0).abs() < 0.01);
        assert!((paris.avg_rating.unwrap() - 4.6).abs() < 0.01);
        assert!((paris.superhost_pct.unwrap() - 35.0).abs() < 0.01);
        assert_eq!(paris.top_property_type.as_deref(), Some("Apartment"));

        let london = &result.locations[1];
        assert_eq!(london.location, "London");
        assert_eq!(london.total_listings, 80);
        assert!((london.avg_price.unwrap() - 200.0).abs() < 0.01);
        assert_eq!(london.top_property_type.as_deref(), Some("Flat"));
    }

    // -----------------------------------------------------------------------
    // Review Sentiment deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn review_sentiment_all_positive() {
        let reviews = vec![
            make_review("Amazing place, truly beautiful and perfect!"),
            make_review("Lovely, excellent stay, wonderful views!"),
            make_review("Great host, clean and comfortable apartment!"),
            make_review("Stunning location, friendly and helpful!"),
            make_review("Perfect stay, beautiful and spacious home!"),
        ];
        let sentiment = compute_review_sentiment("42", &reviews);

        assert_eq!(sentiment.total_reviews_analyzed, 5);
        assert!(
            sentiment.positive_pct > 80.0,
            "All positive reviews should yield > 80% positive, got {}",
            sentiment.positive_pct
        );
        assert!(
            sentiment.negative_pct < 5.0,
            "All positive reviews should yield < 5% negative, got {}",
            sentiment.negative_pct
        );
    }

    #[test]
    fn review_sentiment_mixed() {
        let reviews = vec![
            make_review("Amazing and beautiful place!"),
            make_review("Dirty, noisy, and uncomfortable"),
            make_review("Great location but broken shower"),
            make_review("Nothing special to say about it"),
        ];
        let sentiment = compute_review_sentiment("42", &reviews);

        assert_eq!(sentiment.total_reviews_analyzed, 4);
        // Should have mix of positive, negative, neutral
        assert!(sentiment.positive_pct > 0.0);
        assert!(sentiment.negative_pct > 0.0);
        // "Nothing special" has no positive or negative keywords -> neutral
        assert!(sentiment.neutral_pct > 0.0);
    }

    #[test]
    fn review_sentiment_theme_detection() {
        let reviews = vec![
            make_review("The place was spotless and clean, everything was tidy"),
            make_review("Very clean apartment, no dust anywhere"),
            make_review("Location was great, easy walk to transport"),
        ];
        let sentiment = compute_review_sentiment("42", &reviews);

        // Check cleanliness theme detected via "clean", "spotless", "tidy", "dust"
        let cleanliness = sentiment.themes.iter().find(|t| t.theme == "Cleanliness");
        assert!(
            cleanliness.is_some(),
            "Cleanliness theme should be detected"
        );
        let cleanliness = cleanliness.unwrap();
        assert!(
            cleanliness.mention_count >= 2,
            "Cleanliness should have at least 2 mentions, got {}",
            cleanliness.mention_count
        );

        // Check location theme detected via "location", "walk", "transport"
        let location = sentiment.themes.iter().find(|t| t.theme == "Location");
        assert!(location.is_some(), "Location theme should be detected");
    }

    // -----------------------------------------------------------------------
    // Optimal Pricing deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn optimal_pricing_with_trends() {
        let detail = make_listing_detail("42");
        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(95.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };
        // Create trends with a weekend premium
        let days = vec![
            make_calendar_day("2025-06-06", Some(200.0), true), // Fri (weekend)
            make_calendar_day("2025-06-07", Some(250.0), true), // Sat (weekend)
            make_calendar_day("2025-06-09", Some(100.0), true), // Mon (weekday)
            make_calendar_day("2025-06-10", Some(110.0), true), // Tue (weekday)
        ];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);

        let rec = compute_optimal_pricing(&detail, Some(&stats), Some(&trends), None).unwrap();

        // Should produce weekday/weekend split
        assert!(rec.weekday_recommendation.is_some());
        assert!(rec.weekend_recommendation.is_some());

        // Weekend should be higher than weekday
        let weekday = rec.weekday_recommendation.unwrap();
        let weekend = rec.weekend_recommendation.unwrap();
        assert!(
            weekend > weekday,
            "Weekend ({weekend}) should be higher than weekday ({weekday})"
        );
    }

    #[test]
    fn optimal_pricing_no_data() {
        let detail = make_listing_detail("42"); // price 100

        let rec = compute_optimal_pricing(&detail, None, None, None).unwrap();

        assert_eq!(rec.listing_id, "42");
        assert!(rec.recommended_price > 0.0);
        // With no neighborhood data, baseline = current price = 100
        assert!(
            (rec.recommended_price - 100.0).abs() < 0.01,
            "With no data, recommended should equal current price, got {}",
            rec.recommended_price
        );
        assert!(rec.weekday_recommendation.is_none());
        assert!(rec.weekend_recommendation.is_none());
        assert!(rec.amenity_premium_pct.is_none());
        assert!(rec.vs_neighborhood_median.is_none());
        assert!(!rec.reasoning.is_empty());
    }

    #[test]
    fn optimal_pricing_high_rating_premium() {
        let mut detail = make_listing_detail("42");
        detail.rating = Some(4.95);

        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(100.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: Some("$".into()),
            priced_listings: 0,
        };

        let rec = compute_optimal_pricing(&detail, Some(&stats), None, None).unwrap();

        // Rating diff = 4.95 - 4.5 = 0.45, adjustment = 0.45 * 20 = 9%
        // recommended = 100 (median) + 100 * 9/100 = 109
        assert!(
            rec.recommended_price > 100.0,
            "High-rated listing should be priced above median, got {}",
            rec.recommended_price
        );
        assert!(rec.vs_neighborhood_median.is_some());
        let vs_median = rec.vs_neighborhood_median.unwrap();
        assert!(
            vs_median > 0.0,
            "vs_neighborhood_median should be positive, got {vs_median}"
        );
    }

    /// `n` consecutive days from `start`. Unavailable days get `reason`.
    fn days_from(
        start: &str,
        n: u32,
        available: bool,
        reason: Option<UnavailabilityReason>,
    ) -> Vec<CalendarDay> {
        let first = NaiveDate::parse_from_str(start, "%Y-%m-%d").unwrap();
        let reason = if available { None } else { reason };
        (0..n)
            .map(|i| {
                let date = first + chrono::Days::new(u64::from(i));
                let mut day =
                    make_calendar_day(&date.format("%Y-%m-%d").to_string(), None, available);
                day.unavailability_reason.clone_from(&reason);
                day
            })
            .collect()
    }

    #[test]
    fn price_trends_without_prices_report_unavailable() {
        let days = vec![
            make_calendar_day("2026-10-01", None, true),
            make_calendar_day("2026-10-02", None, false),
            make_calendar_day("2026-11-01", None, true),
        ];
        let trends = compute_price_trends("42", &make_price_calendar("42", days));

        assert!(trends.overall_avg.is_none());
        assert!(trends.overall_min.is_none());
        assert!(trends.price_volatility.is_none());
        assert_eq!(trends.priced_nights, 0);
        assert!(trends.monthly.iter().all(|m| m.avg_price.is_none()));
        assert!(trends.peak_month.is_none());
        let s = trends.to_string();
        assert!(s.contains("Nightly prices: not published"), "{s}");
        assert!(!s.contains("$0"), "{s}");
        assert!(!s.contains("0 $"), "{s}");
    }

    #[test]
    fn price_trends_skip_past_days() {
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from("2026-09-28", 3, true, None));
        let trends = compute_price_trends("42", &make_price_calendar("42", days));

        assert_eq!(trends.monthly.len(), 1);
        assert_eq!(trends.monthly[0].total_days, 3);
        assert_eq!(trends.monthly[0].available_days, 3);
        assert_eq!(trends.period_start, "2026-09-28");
    }

    #[test]
    fn price_trends_display_uses_calendar_currency() {
        let mut cal = make_price_calendar(
            "42",
            vec![
                make_calendar_day("2026-10-02", Some(120.0), true),
                make_calendar_day("2026-10-05", Some(100.0), true),
            ],
        );
        cal.currency = "€".into();
        let s = compute_price_trends("42", &cal).to_string();
        assert!(s.contains("€110"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn occupancy_excludes_past_days_for_listing_without_bookings() {
        // MATH-1 failure scenario: today 2026-09-28, 3-month window from 2026-09-01.
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from("2026-09-28", 64, true, None));
        let est = compute_occupancy_estimate("42", &make_price_calendar("42", days));

        assert_eq!(est.past_days_excluded, 27);
        assert_eq!(est.total_days, 64);
        assert_eq!(est.occupied_days, 0);
        assert!(
            est.occupancy_rate.abs() < 1e-9,
            "got {}",
            est.occupancy_rate
        );
        assert_eq!(est.period_start, "2026-09-28");
        let september = &est.monthly_breakdown[0];
        assert_eq!(
            (
                september.month.as_str(),
                september.total_days,
                september.occupied_days
            ),
            ("2026-09", 3, 0)
        );
    }

    #[test]
    fn occupancy_live_september_counts_only_future_nights() {
        // Live evidence: "Sept 2026 reports 96.7% occupied" because past days counted.
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from(
            "2026-09-28",
            3,
            false,
            Some(UnavailabilityReason::Unknown),
        ));
        let est = compute_occupancy_estimate("42", &make_price_calendar("42", days));

        assert_eq!(est.total_days, 3);
        assert!((est.occupancy_rate - 100.0).abs() < 1e-9);
        assert_eq!(est.monthly_breakdown.len(), 1);
        assert_eq!(est.monthly_breakdown[0].total_days, 3);
    }

    #[test]
    fn occupancy_excludes_host_blocked_days_and_reports_them() {
        let mut days = days_from(
            "2026-10-01",
            2,
            false,
            Some(UnavailabilityReason::BlockedByHost),
        );
        days.extend(days_from(
            "2026-10-03",
            1,
            false,
            Some(UnavailabilityReason::Booked),
        ));
        days.extend(days_from("2026-10-04", 1, true, None));
        let est = compute_occupancy_estimate("42", &make_price_calendar("42", days));

        assert_eq!(est.blocked_days_excluded, 2);
        assert_eq!(est.total_days, 2);
        assert!((est.occupancy_rate - 50.0).abs() < 1e-9);
    }

    #[test]
    fn occupancy_measured_rate_is_none_without_future_nights() {
        let days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        let est = compute_occupancy_estimate("42", &make_price_calendar("42", days));

        assert_eq!(est.total_days, 0);
        assert!(est.measured_rate().is_none());
        assert!(est.to_string().contains("Occupancy rate: unknown"), "{est}");
    }

    #[test]
    fn occupancy_display_labels_upper_bound_and_missing_prices() {
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from(
            "2026-09-28",
            2,
            false,
            Some(UnavailabilityReason::Unknown),
        ));
        days.extend(days_from("2026-09-30", 2, true, None));
        let s = compute_occupancy_estimate("42", &make_price_calendar("42", days)).to_string();

        assert!(s.contains("50.0%"), "{s}");
        assert!(s.contains("upper bound"), "{s}");
        assert!(s.contains("Past days excluded: 27"), "{s}");
        assert!(s.contains("Prices: not published"), "{s}");
        assert!(!s.contains("$0"), "{s}");
    }

    #[test]
    fn occupancy_display_uses_calendar_currency() {
        let mut cal = make_price_calendar(
            "42",
            vec![
                make_calendar_day("2026-10-01", Some(95.0), true),
                make_calendar_day("2026-10-02", Some(95.0), false),
            ],
        );
        cal.currency = "€".into();
        let s = compute_occupancy_estimate("42", &cal).to_string();
        assert!(s.contains("€95"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    fn market_stats(
        location: &str,
        average: Option<f64>,
        median: Option<f64>,
        rating: Option<f64>,
        currency: Option<&str>,
    ) -> NeighborhoodStats {
        NeighborhoodStats {
            location: location.to_string(),
            total_listings: 20,
            average_price: average,
            median_price: median,
            price_range: average.map(|a| (a * 0.5, a * 2.0)),
            average_rating: rating,
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
            currency: currency.map(str::to_string),
            priced_listings: if average.is_some() { 20 } else { 0 },
        }
    }

    #[test]
    fn neighborhood_stats_superhost_unknown_when_no_listing_reports_it() {
        let listings = vec![make_listing("1", "A", 100.0), make_listing("2", "B", 120.0)];
        let stats = compute_neighborhood_stats("Lyon", &listings);
        assert!(stats.superhost_percentage.is_none());
        assert!(stats.to_string().contains("Superhosts: unknown"), "{stats}");
    }

    #[test]
    fn neighborhood_stats_use_listing_currency_and_skip_unpriced() {
        let mut a = make_listing("1", "A", 100.0);
        a.currency = "€".into();
        let mut b = make_listing("2", "B", 200.0);
        b.currency = "€".into();
        let mut c = make_listing("3", "C", 0.0);
        c.currency = "€".into();
        let stats = compute_neighborhood_stats("Lyon", &[a, b, c]);

        assert_eq!(stats.currency.as_deref(), Some("€"));
        assert_eq!(stats.priced_listings, 2);
        let s = stats.to_string();
        assert!(s.contains("Average price: €150/night"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn neighborhood_stats_never_average_across_currencies() {
        let mut a = make_listing("1", "A", 100.0);
        a.currency = "€".into();
        let mut b = make_listing("2", "B", 110.0);
        b.currency = "€".into();
        let mut c = make_listing("3", "C", 60.37);
        c.currency = "$".into();
        let stats = compute_neighborhood_stats("Lyon", &[a, b, c]);

        assert_eq!(stats.currency.as_deref(), Some("€"));
        assert!((stats.average_price.unwrap() - 105.0).abs() < 1e-9);
        assert_eq!(stats.priced_listings, 2);
    }

    #[test]
    fn market_comparison_prints_each_market_currency() {
        let paris = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("€"));
        let tokyo = market_stats("Tokyo", Some(15000.0), Some(14000.0), Some(4.7), Some("¥"));
        let s = compute_market_comparison(&[paris, tokyo]).to_string();
        assert!(s.contains("€150"), "{s}");
        assert!(s.contains("¥15000"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    fn with_amenities(id: &str, amenities: &[&str]) -> ListingDetail {
        let mut detail = make_listing_detail(id);
        detail.amenities = amenities.iter().map(|a| (*a).to_string()).collect();
        detail
    }

    #[test]
    fn amenity_analysis_identical_listings_score_100() {
        let list = [
            "Wifi",
            "Essentials",
            "Towels",
            "Bed linens",
            "TV",
            "Cable TV",
            "Self check-in",
            "Keypad",
            "Kitchen",
            "Washer",
        ];
        let detail = with_amenities("42", &list);
        let neighbors: Vec<ListingDetail> = (1..=5)
            .map(|i| with_amenities(&i.to_string(), &list))
            .collect();
        let analysis = compute_amenity_analysis(&detail, &neighbors);

        assert_eq!(analysis.listing_amenity_count, 6);
        assert!((analysis.neighborhood_avg_amenity_count - 6.0).abs() < 1e-9);
        assert!((analysis.amenity_score_pct.unwrap() - 100.0).abs() < 1e-9);
        assert!(analysis.missing_popular_amenities.is_empty());
    }

    #[test]
    fn amenity_frequency_counts_each_neighbor_once() {
        let detail = with_amenities("42", &["Kitchen"]);
        let mut neighbors = vec![with_amenities(
            "1",
            &["Kitchen", "Essentials", "Towels", "Bed linens"],
        )];
        neighbors.extend((2..=5).map(|i| with_amenities(&i.to_string(), &["Kitchen"])));
        let analysis = compute_amenity_analysis(&detail, &neighbors);
        // 1 of 5 comparables has essentials: 20%, not "popular".
        assert!(
            !analysis
                .missing_popular_amenities
                .iter()
                .any(|a| a.amenity == "essentials"),
            "{:?}",
            analysis.missing_popular_amenities
        );

        let all_five = vec![with_amenities("1", &["Essentials", "Towels", "Bed linens"]); 5];
        let analysis = compute_amenity_analysis(&with_amenities("42", &["Kitchen"]), &all_five);
        let essentials = analysis
            .missing_popular_amenities
            .iter()
            .find(|a| a.amenity == "essentials")
            .unwrap();
        assert!((essentials.neighborhood_frequency_pct - 100.0).abs() < 1e-9);
    }

    #[test]
    fn amenity_analysis_reports_unique_amenities() {
        let detail = with_amenities("42", &["Sauna", "Kitchen"]);
        let neighbors: Vec<ListingDetail> = (1..=5)
            .map(|i| with_amenities(&i.to_string(), &["Kitchen"]))
            .collect();
        let analysis = compute_amenity_analysis(&detail, &neighbors);
        let sauna = analysis
            .present_rare_amenities
            .iter()
            .find(|a| a.amenity == "sauna")
            .unwrap();
        assert!(sauna.neighborhood_frequency_pct.abs() < 1e-9);
    }

    #[test]
    fn amenity_analysis_without_comparables_has_no_score() {
        let detail = make_listing_detail("42");
        let empty_neighbor = with_amenities("7", &[]);
        for neighbors in [vec![], vec![empty_neighbor]] {
            let analysis = compute_amenity_analysis(&detail, &neighbors);
            assert_eq!(analysis.comparables_analyzed, 0);
            assert!(analysis.amenity_score_pct.is_none());
            let s = analysis.to_string();
            assert!(s.contains("Amenity score: unavailable"), "{s}");
        }
    }

    #[test]
    fn listing_score_unknown_price_skips_pricing() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 0.0;
        let stats = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("$"));
        let score = compute_listing_score(&detail, Some(&stats));

        let pricing = score
            .category_scores
            .iter()
            .find(|c| c.category == "Pricing")
            .unwrap();
        assert!(pricing.score.is_none());
        assert!(
            !score
                .suggestions
                .iter()
                .any(|s| s.contains("consider raising")),
            "{:?}",
            score.suggestions
        );
        let scored: Vec<f64> = score
            .category_scores
            .iter()
            .filter_map(|c| c.score)
            .collect();
        assert_eq!(scored.len(), 5);
        assert!((score.overall_score - scored.iter().sum::<f64>() / 5.0).abs() < 1e-9);
    }

    #[test]
    fn listing_score_blends_rating_into_reviews() {
        let reviews_score = |rating: f64, count: u32| {
            let mut detail = make_listing_detail("42");
            detail.rating = Some(rating);
            detail.review_count = count;
            compute_listing_score(&detail, None)
                .category_scores
                .iter()
                .find(|c| c.category == "Reviews")
                .unwrap()
                .score
                .unwrap()
        };
        assert!((reviews_score(3.1, 60) - 50.0).abs() < 1e-9);
        assert!((reviews_score(4.95, 100) - 97.5).abs() < 1e-9);
    }

    #[test]
    fn listing_score_response_rate_is_proportional() {
        let host_score = |rate: &str| {
            let mut detail = make_listing_detail("42");
            detail.host_response_rate = Some(rate.to_string());
            compute_listing_score(&detail, None)
                .category_scores
                .iter()
                .find(|c| c.category == "Host")
                .unwrap()
                .score
                .unwrap()
        };
        assert!((host_score("10%") - 51.0).abs() < 1e-9);
        assert!((host_score("Response rate: 100%") - 60.0).abs() < 1e-9);
    }

    #[test]
    fn listing_score_counts_description_in_characters() {
        let mut detail = make_listing_detail("42");
        detail.description = "日本語の説明".repeat(30); // 180 characters, 540 bytes
        let score = compute_listing_score(&detail, None);
        let description = score
            .category_scores
            .iter()
            .find(|c| c.category == "Description")
            .unwrap();
        assert!((description.score.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(description.details, "180 characters");
    }

    #[test]
    fn listing_score_pricing_uses_listing_currency() {
        let mut detail = make_listing_detail("42");
        detail.currency = "€".into();
        let stats = market_stats("Lyon", Some(100.0), Some(95.0), Some(4.6), Some("€"));
        let s = compute_listing_score(&detail, Some(&stats)).to_string();
        assert!(s.contains("€100/night (market avg: €100)"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn listing_score_pricing_skips_market_in_other_currency() {
        let mut detail = make_listing_detail("42");
        detail.currency = "€".into();
        detail.price_per_night = 120.0;
        let stats = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("$"));
        let score = compute_listing_score(&detail, Some(&stats));

        let pricing = score
            .category_scores
            .iter()
            .find(|c| c.category == "Pricing")
            .unwrap();
        assert!(pricing.score.is_none());
        assert_eq!(
            pricing.details,
            "€120/night (market prices are in another currency)"
        );
        assert!(
            !score
                .suggestions
                .iter()
                .any(|s| s.contains("consider raising") || s.contains("above market")),
            "{:?}",
            score.suggestions
        );
        let scored: Vec<f64> = score
            .category_scores
            .iter()
            .filter_map(|c| c.score)
            .collect();
        assert_eq!(scored.len(), 5);
        assert!((score.overall_score - scored.iter().sum::<f64>() / 5.0).abs() < 1e-9);
    }

    #[test]
    fn revenue_estimate_listing_without_bookings_projects_zero() {
        // MATH-1: past days no longer count as booked nights.
        let mut days = days_from(
            "2026-09-01",
            27,
            false,
            Some(UnavailabilityReason::PastDate),
        );
        days.extend(days_from("2026-09-28", 64, true, None));
        let cal = make_price_calendar("42", days);
        let stats = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("$"));
        let est =
            compute_revenue_estimate(Some("42"), "Paris", None, Some(&cal), Some(&stats)).unwrap();

        assert_eq!(est.occupancy_source, DataSource::Calendar);
        assert_eq!(est.occupancy_nights_measured, 64);
        assert!(est.projected_occupancy_pct.abs() < 1e-9);
        assert!(est.projected_annual_revenue.abs() < 1e-9);
        assert!(
            est.monthly_breakdown
                .iter()
                .all(|m| m.projected_revenue.abs() < 1e-9)
        );
    }

    #[test]
    fn revenue_estimate_location_only_marks_occupancy_as_assumption() {
        let stats = market_stats("Tokyo", Some(120.0), Some(110.0), Some(4.7), Some("¥"));
        let est = compute_revenue_estimate(None, "Tokyo", None, None, Some(&stats)).unwrap();

        assert_eq!(est.occupancy_source, DataSource::Assumption);
        assert!((est.projected_occupancy_pct - ASSUMED_OCCUPANCY_PCT).abs() < 1e-9);
        assert_eq!(est.adr_source, DataSource::NeighborhoodAverage);
        assert_eq!(est.currency, "¥");
        let s = est.to_string();
        assert!(s.contains("ASSUMPTION"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn revenue_estimate_without_any_price_is_insufficient_data() {
        let stats = market_stats("Tokyo", None, None, None, None);
        let err = compute_revenue_estimate(None, "Tokyo", None, None, Some(&stats)).unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    #[test]
    fn revenue_estimate_prefers_listing_price_when_calendar_has_no_prices() {
        let cal = make_price_calendar("42", days_from("2026-10-01", 10, true, None));
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 90.0;
        detail.currency = "€".into();
        let stats = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("€"));
        let est =
            compute_revenue_estimate(Some("42"), "Paris", Some(&detail), Some(&cal), Some(&stats))
                .unwrap();

        assert_eq!(est.adr_source, DataSource::ListingPrice);
        assert!((est.projected_adr - 90.0).abs() < 1e-9);
        assert_eq!(est.currency, "€");
        assert!((est.vs_neighborhood_avg_price_pct.unwrap() + 40.0).abs() < 1e-9);
    }

    #[test]
    fn revenue_estimate_empty_future_window_is_insufficient_data() {
        let cal = make_price_calendar(
            "42",
            days_from(
                "2026-09-01",
                27,
                false,
                Some(UnavailabilityReason::PastDate),
            ),
        );
        let stats = market_stats("Paris", Some(150.0), Some(140.0), Some(4.6), Some("$"));
        let err = compute_revenue_estimate(Some("42"), "Paris", None, Some(&cal), Some(&stats))
            .unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    fn weekend_trends(weekend: f64, weekday: f64) -> PriceTrends {
        // 2025-06-06 = Fri, 06-07 = Sat, 06-09 = Mon, 06-10 = Tue
        let days = vec![
            make_calendar_day("2025-06-06", Some(weekend), true),
            make_calendar_day("2025-06-07", Some(weekend), true),
            make_calendar_day("2025-06-09", Some(weekday), true),
            make_calendar_day("2025-06-10", Some(weekday), true),
        ];
        compute_price_trends("42", &make_price_calendar("42", days))
    }

    #[test]
    fn optimal_pricing_split_keeps_weekly_mean_and_premium() {
        let detail = make_listing_detail("42"); // known price 100
        let rec = compute_optimal_pricing(&detail, None, Some(&weekend_trends(120.0, 100.0)), None)
            .unwrap();
        let weekday = rec.weekday_recommendation.unwrap();
        let weekend = rec.weekend_recommendation.unwrap();
        assert!((weekend / weekday - 1.2).abs() < 1e-9);
        assert!(((5.0 * weekday + 2.0 * weekend) / 7.0 - rec.recommended_price).abs() < 1e-9);
    }

    #[test]
    fn optimal_pricing_extreme_premium_never_goes_negative() {
        let detail = make_listing_detail("42");
        let rec = compute_optimal_pricing(&detail, None, Some(&weekend_trends(330.0, 100.0)), None)
            .unwrap();
        assert!(rec.weekday_recommendation.unwrap() > 0.0);
    }

    #[test]
    fn optimal_pricing_without_median_or_price_is_insufficient_data() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 0.0;
        let err = compute_optimal_pricing(&detail, None, None, None).unwrap_err();
        assert!(matches!(err, AirbnbError::InsufficientData { .. }), "{err}");
    }

    #[test]
    fn optimal_pricing_unknown_current_price_uses_median_and_says_so() {
        let mut detail = make_listing_detail("42"); // rating 4.8
        detail.price_per_night = 0.0;
        detail.currency = "€".into();
        let stats = market_stats("Lyon", Some(130.0), Some(120.0), Some(4.6), Some("€"));
        let rec = compute_optimal_pricing(&detail, Some(&stats), None, None).unwrap();

        assert!(rec.current_price.is_none());
        assert!((rec.recommended_price - 124.8).abs() < 1e-9);
        assert_eq!(rec.currency, "€");
        let s = rec.to_string();
        assert!(s.contains("Current Price: unknown"), "{s}");
        assert!(s.contains("€124.80"), "{s}");
        assert!(!s.contains('$'), "{s}");
    }

    #[test]
    fn optimal_pricing_labels_current_price_in_its_own_currency() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 120.0;
        detail.currency = "€".into();
        detail.rating = None;
        let stats = market_stats("Paris", Some(140.0), Some(150.0), None, Some("$"));
        let rec = compute_optimal_pricing(&detail, Some(&stats), None, None).unwrap();

        assert_eq!(rec.current_price, Some(120.0));
        assert_eq!(rec.current_price_currency, "€");
        assert_eq!(rec.currency, "$");
        let s = rec.to_string();
        assert!(s.contains("Current Price: €120.00/night"), "{s}");
        assert!(s.contains("Recommended Price: $150.00/night"), "{s}");
        assert!(!s.contains("$120.00"), "{s}");
        assert!(
            rec.reasoning.iter().any(|r| r.contains("not converted")),
            "{:?}",
            rec.reasoning
        );
    }

    #[test]
    fn optimal_pricing_skips_amenity_adjustment_without_comparables() {
        let detail = make_listing_detail("42");
        let analysis = compute_amenity_analysis(&detail, &[]);
        let rec = compute_optimal_pricing(&detail, None, None, Some(&analysis)).unwrap();
        assert!(rec.amenity_premium_pct.is_none());
        assert!(
            rec.reasoning.iter().any(|r| r.contains("skipped")),
            "{:?}",
            rec.reasoning
        );
        assert!((rec.recommended_price - 100.0).abs() < 1e-9);
    }

    fn comparable(id: &str, price: f64, rating: Option<f64>, reviews: u32) -> Listing {
        let mut listing = make_listing(id, "Comparable", price);
        listing.rating = rating;
        listing.review_count = reviews;
        listing
    }

    #[test]
    fn positioning_ranks_price_against_comparables_without_saturating() {
        let detail = make_listing_detail("42"); // price 100
        let comps = vec![
            comparable("1", 80.0, Some(4.5), 10),
            comparable("2", 100.0, Some(4.6), 20),
            comparable("3", 120.0, Some(4.7), 30),
        ];
        let result = compute_competitive_positioning(&detail, &comps, None, None);
        let price = result
            .axes
            .iter()
            .find(|a| a.axis == "Price Value")
            .unwrap();
        assert!((price.percentile.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(price.assessment, "Fair value");
        assert_eq!(price.sample_size, 3);
        assert!((price.neighborhood_avg.unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn positioning_price_above_every_comparable_is_premium_not_strong_value() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 130.0;
        let comps = vec![
            comparable("1", 80.0, None, 10),
            comparable("2", 100.0, None, 20),
            comparable("3", 120.0, None, 30),
        ];
        let result = compute_competitive_positioning(&detail, &comps, None, None);
        let price = result
            .axes
            .iter()
            .find(|a| a.axis == "Price Value")
            .unwrap();
        assert!(price.percentile.unwrap().abs() < 1e-9);
        assert_eq!(price.assessment, "Premium priced");
        assert!(result.weaknesses.contains(&"Price Value".to_string()));
        assert!(!result.strengths.contains(&"Price Value".to_string()));
    }

    #[test]
    fn positioning_display_has_no_ordinal_suffix_bug() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 130.0;
        let comps = vec![
            comparable("1", 100.0, None, 10),
            comparable("2", 120.0, None, 20),
            comparable("3", 140.0, None, 30),
        ];
        let s = compute_competitive_positioning(&detail, &comps, None, None).to_string();
        assert!(s.contains("ranks above 33% of 3 comparables"), "{s}");
        assert!(!s.contains("33th"), "{s}");
        assert!(!s.contains("th percentile"), "{s}");
    }

    #[test]
    fn positioning_uses_real_neighborhood_figures_not_constants() {
        let detail = make_listing_detail("42"); // 25 reviews, rating 4.8, price 100
        let comps = vec![
            comparable("1", 80.0, Some(4.5), 10),
            comparable("2", 100.0, Some(4.6), 20),
            comparable("3", 120.0, Some(4.7), 30),
        ];
        let mut days = days_from("2026-10-01", 4, false, Some(UnavailabilityReason::Unknown));
        days.extend(days_from("2026-10-05", 1, true, None));
        let occupancy = compute_occupancy_estimate("42", &make_price_calendar("42", days));
        let result = compute_competitive_positioning(&detail, &comps, Some(&occupancy), None);

        let reviews = result
            .axes
            .iter()
            .find(|a| a.axis == "Review Volume")
            .unwrap();
        assert!((reviews.neighborhood_avg.unwrap() - 20.0).abs() < 1e-9);
        assert!((reviews.percentile.unwrap() - 200.0 / 3.0).abs() < 1e-9);
        let occupancy_axis = result.axes.iter().find(|a| a.axis == "Occupancy").unwrap();
        assert!(occupancy_axis.percentile.is_none());
        assert!(occupancy_axis.neighborhood_avg.is_none());
        assert!((occupancy_axis.listing_value.unwrap() - 80.0).abs() < 1e-9);
        // Overall = mean of the ranked axes: price 50, rating 100, reviews 66.7.
        let expected = (50.0 + 100.0 + 200.0 / 3.0) / 3.0;
        assert!((result.overall_competitiveness.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn positioning_unknown_price_is_not_ranked() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 0.0;
        let comps = vec![comparable("1", 80.0, Some(4.5), 10)];
        let result = compute_competitive_positioning(&detail, &comps, None, None);
        let price = result
            .axes
            .iter()
            .find(|a| a.axis == "Price Value")
            .unwrap();
        assert!(price.percentile.is_none());
        assert!(price.listing_value.is_none());
        assert!(!result.strengths.contains(&"Price Value".to_string()));
    }

    #[test]
    fn positioning_without_amenity_comparables_is_not_ranked() {
        let detail = make_listing_detail("42");
        let analysis = compute_amenity_analysis(&detail, &[]);
        let result = compute_competitive_positioning(&detail, &[], None, Some(&analysis));
        let amenity = result
            .axes
            .iter()
            .find(|a| a.axis == "Amenity Count")
            .unwrap();
        assert!(amenity.percentile.is_none());
        assert!(!result.strengths.contains(&"Amenity Count".to_string()));
        assert!(result.overall_competitiveness.is_none());
        assert!(result.to_string().contains("not computable"));
    }

    #[test]
    fn sentiment_matches_whole_words_only() {
        let s = compute_review_sentiment(
            "42",
            &[make_review(
                "Lovely host, she left chocolates and plates for us",
            )],
        );
        assert!((s.positive_pct - 100.0).abs() < 1e-9, "{s}");
        assert!(
            s.top_negative_keywords.is_empty(),
            "{:?}",
            s.top_negative_keywords
        );
    }

    #[test]
    fn sentiment_uncomfortable_and_unfriendly_are_negative() {
        let s = compute_review_sentiment(
            "42",
            &[
                make_review("The bed was uncomfortable."),
                make_review("The host was unfriendly and rude"),
            ],
        );
        assert!((s.negative_pct - 100.0).abs() < 1e-9, "{s}");
    }

    #[test]
    fn sentiment_handles_simple_negation() {
        let s = compute_review_sentiment(
            "42",
            &[
                make_review("The place was dirty, not great, not clean"),
                make_review("Nothing was broken, never cold, not noisy at all."),
            ],
        );
        assert!((s.negative_pct - 50.0).abs() < 1e-9, "{s}");
        assert!((s.positive_pct - 50.0).abs() < 1e-9, "{s}");
        assert!(
            s.top_negative_keywords
                .iter()
                .any(|(k, _)| k == "not clean"),
            "{:?}",
            s.top_negative_keywords
        );
    }

    #[test]
    fn sentiment_skips_non_english_reviews() {
        let mut french = make_review("Appartement super, très propre et bien situé");
        french.language = Some("fr".into());
        let mut translated = make_review("Great apartment, very clean");
        translated.language = Some("fr".into());
        translated.is_translated = Some(true);
        let s = compute_review_sentiment("42", &[french, translated, make_review("Lovely stay")]);

        assert_eq!(s.total_reviews_analyzed, 2);
        assert_eq!(s.skipped_non_english, 1);
        assert!((s.positive_pct - 100.0).abs() < 1e-9, "{s}");
        assert!(s.to_string().contains("1 non-English"), "{s}");
    }

    #[test]
    fn sentiment_assigns_theme_polarity_by_clause() {
        let s =
            compute_review_sentiment("42", &[make_review("Great location. The room was dirty.")]);
        let location = s.themes.iter().find(|t| t.theme == "Location").unwrap();
        let cleanliness = s.themes.iter().find(|t| t.theme == "Cleanliness").unwrap();
        assert_eq!((location.positive_count, location.negative_count), (1, 0));
        assert_eq!(
            (cleanliness.positive_count, cleanliness.negative_count),
            (0, 1)
        );
    }
}
