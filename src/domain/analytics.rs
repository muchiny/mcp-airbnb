#![allow(clippy::cast_precision_loss)] // Counts are small enough for f64

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use super::calendar::PriceCalendar;
use super::listing::{Listing, ListingDetail};
use super::review::Review;

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
    pub superhost_percentage: Option<f64>,
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
    pub period_start: String,
    pub period_end: String,
    pub total_days: u32,
    pub occupied_days: u32,
    pub available_days: u32,
    pub occupancy_rate: f64,
    pub average_available_price: Option<f64>,
    pub weekend_avg_price: Option<f64>,
    pub weekday_avg_price: Option<f64>,
    pub monthly_breakdown: Vec<MonthlyOccupancy>,
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
    /// Percentile rank for price (0-100, lower = cheaper)
    pub price_percentile: f64,
    /// Percentile rank for rating (0-100, higher = better)
    pub rating_percentile: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub count: u32,
    pub avg_price: f64,
    pub median_price: f64,
    pub avg_rating: Option<f64>,
    pub price_range: (f64, f64),
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
    pub avg_price: f64,
    pub min_price: f64,
    pub max_price: f64,
    pub weekend_avg: Option<f64>,
    pub weekday_avg: Option<f64>,
    pub available_days: u32,
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
    pub overall_avg: f64,
    pub overall_min: f64,
    pub overall_max: f64,
    /// Standard deviation / mean (coefficient of variation)
    pub price_volatility: f64,
    /// `(weekend_avg - weekday_avg) / weekday_avg * 100`
    pub weekend_premium_pct: Option<f64>,
    pub peak_month: Option<String>,
    pub off_peak_month: Option<String>,
    pub monthly: Vec<MonthlyPriceSummary>,
    pub day_of_week: Vec<DayOfWeekPrice>,
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
    /// "orphan" (1 night), "short gap" (2-3), "gap" (4+)
    pub gap_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapFinderResult {
    pub listing_id: String,
    pub total_gaps: u32,
    pub total_gap_nights: u32,
    pub orphan_nights: u32,
    pub short_gaps: u32,
    pub potential_lost_revenue: Option<f64>,
    pub gaps: Vec<CalendarGap>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueEstimate {
    pub listing_id: Option<String>,
    pub location: String,
    pub projected_adr: f64,
    pub projected_occupancy_pct: f64,
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
    pub score: f64,
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
    pub listing_amenity_count: u32,
    pub neighborhood_avg_amenity_count: f64,
    pub missing_popular_amenities: Vec<AmenityGap>,
    pub present_rare_amenities: Vec<AmenityGap>,
    pub amenity_score_pct: f64,
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
    pub price_per_night: f64,
    pub rating: Option<f64>,
    pub review_count: u32,
    pub property_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPortfolio {
    pub host_name: String,
    pub host_id: Option<String>,
    pub total_properties: u32,
    pub avg_rating: Option<f64>,
    pub avg_price: f64,
    pub price_range: (f64, f64),
    pub total_reviews: u32,
    pub is_superhost: Option<bool>,
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
}

// ---------------------------------------------------------------------------
// Competitive Positioning types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitiveAxis {
    pub axis: String,
    pub listing_value: f64,
    pub neighborhood_avg: f64,
    pub percentile: f64,
    pub assessment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitivePositioning {
    pub listing_id: String,
    pub axes: Vec<CompetitiveAxis>,
    pub overall_competitiveness: f64,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
}

// ---------------------------------------------------------------------------
// Optimal Pricing types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRecommendation {
    pub listing_id: String,
    pub current_price: f64,
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
        if let Some(avg) = self.average_price {
            writeln!(f, "Average price: ${avg:.0}/night")?;
        }
        if let Some(med) = self.median_price {
            writeln!(f, "Median price: ${med:.0}/night")?;
        }
        if let Some((min, max)) = self.price_range {
            writeln!(f, "Price range: ${min:.0} - ${max:.0}/night")?;
        }
        if let Some(rating) = self.average_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        if let Some(pct) = self.superhost_percentage {
            writeln!(f, "Superhosts: {pct:.0}%")?;
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
        writeln!(f, "Period: {} to {}", self.period_start, self.period_end)?;
        writeln!(
            f,
            "Days: {} total, {} occupied, {} available",
            self.total_days, self.occupied_days, self.available_days
        )?;
        writeln!(f, "Occupancy rate: {:.1}%", self.occupancy_rate)?;
        if let Some(avg) = self.average_available_price {
            writeln!(f, "Avg available price: ${avg:.0}/night")?;
        }
        if let Some(we) = self.weekend_avg_price {
            writeln!(f, "Weekend avg: ${we:.0}/night")?;
        }
        if let Some(wd) = self.weekday_avg_price {
            writeln!(f, "Weekday avg: ${wd:.0}/night")?;
        }
        if !self.monthly_breakdown.is_empty() {
            writeln!(f, "\nMonthly breakdown:")?;
            writeln!(
                f,
                "{:<10} {:>6} {:>8} {:>8} {:>10} {:>10}",
                "Month", "Days", "Occupied", "Avail", "Occ%", "Avg price"
            )?;
            for m in &self.monthly_breakdown {
                let price = m
                    .average_price
                    .map_or_else(|| "-".to_string(), |p| format!("${p:.0}"));
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
        writeln!(
            f,
            "Summary: avg ${:.0}/night, median ${:.0}/night, range ${:.0}-${:.0}",
            self.summary.avg_price,
            self.summary.median_price,
            self.summary.price_range.0,
            self.summary.price_range.1,
        )?;
        if let Some(rating) = self.summary.avg_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        writeln!(f, "Superhosts: {}\n", self.summary.superhost_count)?;
        writeln!(
            f,
            "{:<8} {:<30} {:>10} {:>8} {:>10} {:>12} {:>10}",
            "ID", "Name", "Price", "Rating", "Reviews", "Type", "Price%"
        )?;
        writeln!(f, "{}", "-".repeat(90))?;
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
            writeln!(
                f,
                "{:<8} {:<30} {:>8}{:>2} {:>8} {:>10} {:>12} {:>9.0}%",
                l.id,
                name,
                format!("{:.0}", l.price_per_night),
                l.currency,
                rating,
                l.review_count,
                ptype,
                l.price_percentile,
            )?;
        }
        Ok(())
    }
}

impl std::fmt::Display for PriceTrends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Price Trends: listing {}", self.listing_id)?;
        writeln!(f, "Period: {} to {}", self.period_start, self.period_end)?;
        writeln!(
            f,
            "Overall: avg {}{:.0}, min {}{:.0}, max {}{:.0}",
            self.currency,
            self.overall_avg,
            self.currency,
            self.overall_min,
            self.currency,
            self.overall_max
        )?;
        writeln!(f, "Price volatility: {:.1}%", self.price_volatility * 100.0)?;
        if let Some(prem) = self.weekend_premium_pct {
            writeln!(f, "Weekend premium: {prem:+.1}%")?;
        }
        if let Some(ref peak) = self.peak_month {
            writeln!(f, "Peak month: {peak}")?;
        }
        if let Some(ref off) = self.off_peak_month {
            writeln!(f, "Off-peak month: {off}")?;
        }
        if !self.monthly.is_empty() {
            writeln!(f, "\nMonthly breakdown:")?;
            writeln!(
                f,
                "{:<10} {:>8} {:>8} {:>8} {:>10} {:>10} {:>6}/{:>6}",
                "Month", "Avg", "Min", "Max", "WE avg", "WD avg", "Avail", "Total"
            )?;
            for m in &self.monthly {
                let we = m
                    .weekend_avg
                    .map_or_else(|| "-".to_string(), |p| format!("{}{p:.0}", self.currency));
                let wd = m
                    .weekday_avg
                    .map_or_else(|| "-".to_string(), |p| format!("{}{p:.0}", self.currency));
                writeln!(
                    f,
                    "{:<10} {:>6}{:>2} {:>6}{:>2} {:>6}{:>2} {:>10} {:>10} {:>6}/{:>6}",
                    m.month,
                    format!("{:.0}", m.avg_price),
                    self.currency,
                    format!("{:.0}", m.min_price),
                    self.currency,
                    format!("{:.0}", m.max_price),
                    self.currency,
                    we,
                    wd,
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
                    "  {:<10} {}{:.0} ({} days)",
                    d.day, self.currency, d.avg_price, d.sample_count
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
            "Found {} gaps ({} total gap nights)",
            self.total_gaps, self.total_gap_nights
        )?;
        writeln!(f, "Orphan nights (1-night): {}", self.orphan_nights)?;
        writeln!(f, "Short gaps (2-3 nights): {}", self.short_gaps)?;
        if let Some(rev) = self.potential_lost_revenue {
            writeln!(f, "Potential lost revenue: ${rev:.0}")?;
        }
        if let Some(min) = self.suggested_min_nights {
            writeln!(f, "Suggested minimum nights: {min}")?;
        }
        if !self.gaps.is_empty() {
            writeln!(f, "\nGaps:")?;
            for g in &self.gaps {
                let rev = g
                    .potential_revenue
                    .map_or_else(String::new, |r| format!(" (${r:.0} potential)"));
                writeln!(
                    f,
                    "  {} to {} — {} night(s) [{}]{rev}",
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
        writeln!(
            f,
            "Projected ADR: {}{:.0}/night",
            self.currency, self.projected_adr
        )?;
        writeln!(
            f,
            "Projected occupancy: {:.1}%",
            self.projected_occupancy_pct
        )?;
        writeln!(
            f,
            "Projected monthly revenue: {}{:.0}",
            self.currency, self.projected_monthly_revenue
        )?;
        writeln!(
            f,
            "Projected annual revenue: {}{:.0}",
            self.currency, self.projected_annual_revenue
        )?;
        if let Some(pct) = self.vs_neighborhood_avg_price_pct {
            writeln!(f, "vs neighborhood avg price: {pct:+.1}%")?;
        }
        if !self.monthly_breakdown.is_empty() {
            writeln!(f, "\nMonthly projection:")?;
            writeln!(
                f,
                "{:<10} {:>12} {:>10} {:>12}",
                "Month", "Revenue", "Occ%", "Avg rate"
            )?;
            for m in &self.monthly_breakdown {
                writeln!(
                    f,
                    "{:<10} {:>10}{:>2} {:>9.1}% {:>10}{:>2}",
                    m.month,
                    format!("{:.0}", m.projected_revenue),
                    self.currency,
                    m.projected_occupancy_pct,
                    format!("{:.0}", m.avg_nightly_rate),
                    self.currency,
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
            writeln!(
                f,
                "  {}: {:.0}/100 — {}",
                cat.category, cat.score, cat.details
            )?;
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
        writeln!(
            f,
            "Your amenities: {} (neighborhood avg: {:.0})",
            self.listing_amenity_count, self.neighborhood_avg_amenity_count
        )?;
        writeln!(f, "Amenity score: {:.0}%\n", self.amenity_score_pct)?;
        if !self.missing_popular_amenities.is_empty() {
            writeln!(f, "Missing popular amenities:")?;
            for a in &self.missing_popular_amenities {
                writeln!(
                    f,
                    "  - {} ({:.0}% of competitors have it)",
                    a.amenity, a.neighborhood_frequency_pct
                )?;
            }
        }
        if !self.present_rare_amenities.is_empty() {
            writeln!(f, "\nYour unique/rare amenities:")?;
            for a in &self.present_rare_amenities {
                writeln!(
                    f,
                    "  + {} (only {:.0}% of competitors)",
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
            let avg = loc
                .avg_price
                .map_or_else(|| "-".to_string(), |p| format!("${p:.0}"));
            let med = loc
                .median_price
                .map_or_else(|| "-".to_string(), |p| format!("${p:.0}"));
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
        writeln!(f, "Total properties: {}", self.total_properties)?;
        if let Some(rating) = self.avg_rating {
            writeln!(f, "Average rating: {rating:.2}")?;
        }
        writeln!(
            f,
            "Average price: ${:.0}/night (range: ${:.0}-${:.0})",
            self.avg_price, self.price_range.0, self.price_range.1
        )?;
        writeln!(f, "Total reviews: {}", self.total_reviews)?;
        if !self.properties.is_empty() {
            writeln!(f, "\nProperties:")?;
            for (i, p) in self.properties.iter().enumerate() {
                let rating = p
                    .rating
                    .map_or_else(|| "-".to_string(), |r| format!("{r:.1}"));
                let ptype = p.property_type.as_deref().unwrap_or("-");
                writeln!(
                    f,
                    "  {}. {} (ID: {}) — {} — ${:.0}/night, {rating} ({} reviews) [{}]",
                    i + 1,
                    p.name,
                    p.id,
                    p.location,
                    p.price_per_night,
                    p.review_count,
                    ptype,
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
        writeln!(f, "Reviews Analyzed: {}", self.total_reviews_analyzed)?;
        writeln!(
            f,
            "Sentiment: {:.0}% positive, {:.0}% negative, {:.0}% neutral",
            self.positive_pct, self.negative_pct, self.neutral_pct
        )?;
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
        writeln!(
            f,
            "Overall Competitiveness: {:.0}/100",
            self.overall_competitiveness
        )?;
        if !self.axes.is_empty() {
            writeln!(f, "\n--- Axes ---")?;
            for axis in &self.axes {
                writeln!(
                    f,
                    "{}: {:.1} (avg: {:.1}) — {:.0}th percentile — {}",
                    axis.axis,
                    axis.listing_value,
                    axis.neighborhood_avg,
                    axis.percentile,
                    axis.assessment
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
        writeln!(f, "Current Price: ${:.2}/night", self.current_price)?;
        writeln!(f, "Recommended Price: ${:.2}/night", self.recommended_price)?;
        writeln!(
            f,
            "Recommended Range: ${:.2} - ${:.2}/night",
            self.recommended_range.0, self.recommended_range.1
        )?;
        if let (Some(weekday), Some(weekend)) =
            (self.weekday_recommendation, self.weekend_recommendation)
        {
            writeln!(f, "\nWeekday: ${weekday:.2}  |  Weekend: ${weekend:.2}")?;
        }
        if let Some(prem) = self.amenity_premium_pct {
            writeln!(f, "Amenity Premium: {prem:.0}%")?;
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

#[allow(clippy::cast_possible_truncation)]
pub fn compute_neighborhood_stats(location: &str, listings: &[Listing]) -> NeighborhoodStats {
    let total_listings = listings.len() as u32;

    // Prices (exclude zero-price listings from incomplete data sources like CSS fallback)
    let mut prices: Vec<f64> = listings
        .iter()
        .map(|l| l.price_per_night)
        .filter(|&p| p > 0.0)
        .collect();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let average_price = if prices.is_empty() {
        None
    } else {
        Some(prices.iter().sum::<f64>() / prices.len() as f64)
    };

    let median_price = if prices.is_empty() {
        None
    } else {
        let mid = prices.len() / 2;
        if prices.len().is_multiple_of(2) {
            Some(f64::midpoint(prices[mid - 1], prices[mid]))
        } else {
            Some(prices[mid])
        }
    };

    let price_range = if prices.is_empty() {
        None
    } else {
        Some((prices[0], prices[prices.len() - 1]))
    };

    // Ratings
    let ratings: Vec<f64> = listings.iter().filter_map(|l| l.rating).collect();
    let average_rating = if ratings.is_empty() {
        None
    } else {
        Some(ratings.iter().sum::<f64>() / ratings.len() as f64)
    };

    // Property type distribution
    let mut type_counts: HashMap<String, u32> = HashMap::new();
    for listing in listings {
        let pt = listing
            .property_type
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        *type_counts.entry(pt).or_insert(0) += 1;
    }
    let mut property_type_distribution: Vec<PropertyTypeCount> = type_counts
        .into_iter()
        .map(|(property_type, count)| {
            let percentage = if total_listings > 0 {
                f64::from(count) / f64::from(total_listings) * 100.0
            } else {
                0.0
            };
            PropertyTypeCount {
                property_type,
                count,
                percentage,
            }
        })
        .collect();
    property_type_distribution.sort_by(|a, b| b.count.cmp(&a.count));

    // Superhost percentage
    let superhost_count = listings
        .iter()
        .filter(|l| l.is_superhost == Some(true))
        .count();
    let superhost_percentage = if total_listings > 0 {
        Some(superhost_count as f64 / f64::from(total_listings) * 100.0)
    } else {
        None
    };

    NeighborhoodStats {
        location: location.to_string(),
        total_listings,
        average_price,
        median_price,
        price_range,
        average_rating,
        property_type_distribution,
        superhost_percentage,
    }
}

#[allow(clippy::cast_possible_truncation)]
pub fn compute_occupancy_estimate(listing_id: &str, calendar: &PriceCalendar) -> OccupancyEstimate {
    let days = &calendar.days;

    let total_days = days.len() as u32;
    let available_days = days.iter().filter(|d| d.available).count() as u32;
    let occupied_days = total_days - available_days;
    let occupancy_rate = if total_days > 0 {
        f64::from(occupied_days) / f64::from(total_days) * 100.0
    } else {
        0.0
    };

    // Available prices
    let available_prices: Vec<f64> = days
        .iter()
        .filter(|d| d.available)
        .filter_map(|d| d.price)
        .collect();
    let average_available_price = if available_prices.is_empty() {
        None
    } else {
        Some(available_prices.iter().sum::<f64>() / available_prices.len() as f64)
    };

    // Weekend vs weekday prices (among available days with prices)
    let mut weekend_prices = Vec::new();
    let mut weekday_prices = Vec::new();
    for day in days.iter().filter(|d| d.available) {
        if let Some(price) = day.price
            && let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
        {
            match date.weekday() {
                Weekday::Fri | Weekday::Sat => weekend_prices.push(price),
                _ => weekday_prices.push(price),
            }
        }
    }
    let weekend_avg_price = if weekend_prices.is_empty() {
        None
    } else {
        Some(weekend_prices.iter().sum::<f64>() / weekend_prices.len() as f64)
    };
    let weekday_avg_price = if weekday_prices.is_empty() {
        None
    } else {
        Some(weekday_prices.iter().sum::<f64>() / weekday_prices.len() as f64)
    };

    // Period
    let period_start = days.first().map_or_else(String::new, |d| d.date.clone());
    let period_end = days.last().map_or_else(String::new, |d| d.date.clone());

    // Monthly breakdown
    let mut monthly: HashMap<String, (u32, u32, Vec<f64>)> = HashMap::new();
    for day in days {
        let month_key = if day.date.len() >= 7 {
            day.date[..7].to_string()
        } else {
            "unknown".to_string()
        };
        let entry = monthly.entry(month_key).or_insert((0, 0, Vec::new()));
        entry.0 += 1; // total
        if !day.available {
            entry.1 += 1; // occupied
        }
        if day.available
            && let Some(price) = day.price
        {
            entry.2.push(price);
        }
    }
    let mut monthly_breakdown: Vec<MonthlyOccupancy> = monthly
        .into_iter()
        .map(|(month, (total, occupied, prices))| {
            let avail = total - occupied;
            let occ_rate = if total > 0 {
                f64::from(occupied) / f64::from(total) * 100.0
            } else {
                0.0
            };
            let avg_price = if prices.is_empty() {
                None
            } else {
                Some(prices.iter().sum::<f64>() / prices.len() as f64)
            };
            MonthlyOccupancy {
                month,
                total_days: total,
                occupied_days: occupied,
                available_days: avail,
                occupancy_rate: occ_rate,
                average_price: avg_price,
            }
        })
        .collect();
    monthly_breakdown.sort_by(|a, b| a.month.cmp(&b.month));

    OccupancyEstimate {
        listing_id: listing_id.to_string(),
        period_start,
        period_end,
        total_days,
        occupied_days,
        available_days,
        occupancy_rate,
        average_available_price,
        weekend_avg_price,
        weekday_avg_price,
        monthly_breakdown,
    }
}

// ---------------------------------------------------------------------------
// Price Trends computation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn compute_price_trends(listing_id: &str, calendar: &PriceCalendar) -> PriceTrends {
    let days = &calendar.days;
    let available_with_price: Vec<_> = days
        .iter()
        .filter(|d| d.available && d.price.is_some())
        .collect();

    let prices: Vec<f64> = available_with_price
        .iter()
        .filter_map(|d| d.price)
        .collect();
    let overall_avg = if prices.is_empty() {
        0.0
    } else {
        prices.iter().sum::<f64>() / prices.len() as f64
    };
    let overall_min = prices.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let overall_max = prices.iter().copied().reduce(f64::max).unwrap_or(0.0);

    // Coefficient of variation (std_dev / mean)
    let price_volatility = if prices.len() > 1 && overall_avg > 0.0 {
        let variance = prices
            .iter()
            .map(|p| (p - overall_avg).powi(2))
            .sum::<f64>()
            / prices.len() as f64;
        variance.sqrt() / overall_avg
    } else {
        0.0
    };

    // Weekend vs weekday
    let mut weekend_prices = Vec::new();
    let mut weekday_prices = Vec::new();
    for day in &available_with_price {
        if let Some(price) = day.price
            && let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
        {
            match date.weekday() {
                Weekday::Fri | Weekday::Sat => weekend_prices.push(price),
                _ => weekday_prices.push(price),
            }
        }
    }
    let weekend_avg = if weekend_prices.is_empty() {
        None
    } else {
        Some(weekend_prices.iter().sum::<f64>() / weekend_prices.len() as f64)
    };
    let weekday_avg = if weekday_prices.is_empty() {
        None
    } else {
        Some(weekday_prices.iter().sum::<f64>() / weekday_prices.len() as f64)
    };
    let weekend_premium_pct = match (weekend_avg, weekday_avg) {
        (Some(we), Some(wd)) if wd > 0.0 => Some((we - wd) / wd * 100.0),
        _ => None,
    };

    // Monthly breakdown
    let mut monthly_data: HashMap<String, Vec<&super::calendar::CalendarDay>> = HashMap::new();
    for day in days {
        if day.date.len() >= 7 {
            monthly_data
                .entry(day.date[..7].to_string())
                .or_default()
                .push(day);
        }
    }
    let mut monthly: Vec<MonthlyPriceSummary> = monthly_data
        .into_iter()
        .map(|(month, month_days)| {
            let total_days = month_days.len() as u32;
            let avail_prices: Vec<f64> = month_days
                .iter()
                .filter(|d| d.available)
                .filter_map(|d| d.price)
                .collect();
            let available_days = month_days.iter().filter(|d| d.available).count() as u32;
            let avg_price = if avail_prices.is_empty() {
                0.0
            } else {
                avail_prices.iter().sum::<f64>() / avail_prices.len() as f64
            };
            let min_price = avail_prices.iter().copied().reduce(f64::min).unwrap_or(0.0);
            let max_price = avail_prices.iter().copied().reduce(f64::max).unwrap_or(0.0);

            let mut we_prices = Vec::new();
            let mut wd_prices = Vec::new();
            for d in &month_days {
                if d.available
                    && let Some(price) = d.price
                    && let Ok(date) = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                {
                    match date.weekday() {
                        Weekday::Fri | Weekday::Sat => we_prices.push(price),
                        _ => wd_prices.push(price),
                    }
                }
            }
            let weekend_avg = if we_prices.is_empty() {
                None
            } else {
                Some(we_prices.iter().sum::<f64>() / we_prices.len() as f64)
            };
            let weekday_avg = if wd_prices.is_empty() {
                None
            } else {
                Some(wd_prices.iter().sum::<f64>() / wd_prices.len() as f64)
            };

            MonthlyPriceSummary {
                month,
                avg_price,
                min_price,
                max_price,
                weekend_avg,
                weekday_avg,
                available_days,
                total_days,
            }
        })
        .collect();
    monthly.sort_by(|a, b| a.month.cmp(&b.month));

    // Peak / off-peak
    let peak_month = monthly
        .iter()
        .filter(|m| m.avg_price > 0.0)
        .max_by(|a, b| {
            a.avg_price
                .partial_cmp(&b.avg_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|m| m.month.clone());
    let off_peak_month = monthly
        .iter()
        .filter(|m| m.avg_price > 0.0)
        .min_by(|a, b| {
            a.avg_price
                .partial_cmp(&b.avg_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|m| m.month.clone());

    // Day-of-week breakdown
    let mut dow_data: HashMap<Weekday, Vec<f64>> = HashMap::new();
    for day in &available_with_price {
        if let Some(price) = day.price
            && let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
        {
            dow_data.entry(date.weekday()).or_default().push(price);
        }
    }
    let dow_order = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let day_of_week: Vec<DayOfWeekPrice> = dow_order
        .iter()
        .filter_map(|wd| {
            dow_data.get(wd).map(|prices| DayOfWeekPrice {
                day: format!("{wd}"),
                avg_price: prices.iter().sum::<f64>() / prices.len() as f64,
                sample_count: prices.len() as u32,
            })
        })
        .collect();

    let period_start = days.first().map_or_else(String::new, |d| d.date.clone());
    let period_end = days.last().map_or_else(String::new, |d| d.date.clone());

    PriceTrends {
        listing_id: listing_id.to_string(),
        currency: calendar.currency.clone(),
        period_start,
        period_end,
        overall_avg,
        overall_min,
        overall_max,
        price_volatility,
        weekend_premium_pct,
        peak_month,
        off_peak_month,
        monthly,
        day_of_week,
    }
}

// ---------------------------------------------------------------------------
// Gap Finder computation
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
pub fn compute_gap_finder(listing_id: &str, calendar: &PriceCalendar) -> GapFinderResult {
    let days = &calendar.days;
    let mut gaps = Vec::new();

    // Find sequences of available days between occupied (unavailable) days
    let mut i = 0;
    while i < days.len() {
        // Skip occupied days
        if !days[i].available {
            i += 1;
            continue;
        }
        // Check that this available stretch is bordered by occupied days
        let has_occupied_before = i > 0 && !days[i - 1].available;
        // Find end of available stretch
        let start = i;
        while i < days.len() && days[i].available {
            i += 1;
        }
        let has_occupied_after = i < days.len() && !days[i].available;

        // Only count as gap if bordered by occupied days on both sides
        if has_occupied_before && has_occupied_after {
            let gap_days = &days[start..i];
            let nights = gap_days.len() as u32;
            let gap_prices: Vec<f64> = gap_days.iter().filter_map(|d| d.price).collect();
            let avg_price = if gap_prices.is_empty() {
                None
            } else {
                Some(gap_prices.iter().sum::<f64>() / gap_prices.len() as f64)
            };
            let potential_revenue = avg_price.map(|avg| avg * f64::from(nights));
            let gap_type = match nights {
                1 => "orphan".to_string(),
                2..=3 => "short_gap".to_string(),
                _ => "gap".to_string(),
            };

            gaps.push(CalendarGap {
                start_date: gap_days[0].date.clone(),
                end_date: gap_days[gap_days.len() - 1].date.clone(),
                nights,
                avg_price,
                potential_revenue,
                gap_type,
            });
        }
    }

    let total_gaps = gaps.len() as u32;
    let total_gap_nights: u32 = gaps.iter().map(|g| g.nights).sum();
    let orphan_nights = gaps.iter().filter(|g| g.gap_type == "orphan").count() as u32;
    let short_gaps = gaps.iter().filter(|g| g.gap_type == "short_gap").count() as u32;
    let potential_lost_revenue: Option<f64> = {
        let total: f64 = gaps.iter().filter_map(|g| g.potential_revenue).sum();
        if total > 0.0 { Some(total) } else { None }
    };

    // Suggest minimum nights based on gap patterns
    let suggested_min_nights = if orphan_nights > 0 && orphan_nights >= short_gaps {
        Some(2) // Many orphan nights suggest min 2 would help
    } else if short_gaps > 0 {
        Some(1) // Short gaps suggest lowering min nights to 1
    } else {
        None
    };

    GapFinderResult {
        listing_id: listing_id.to_string(),
        total_gaps,
        total_gap_nights,
        orphan_nights,
        short_gaps,
        potential_lost_revenue,
        gaps,
        suggested_min_nights,
    }
}

// ---------------------------------------------------------------------------
// Revenue Estimate computation
// ---------------------------------------------------------------------------

pub fn compute_revenue_estimate(
    listing_id: Option<&str>,
    location: &str,
    calendar: Option<&PriceCalendar>,
    neighborhood: Option<&NeighborhoodStats>,
    occupancy: Option<&OccupancyEstimate>,
) -> RevenueEstimate {
    // ADR: prefer calendar data, fallback to neighborhood average
    let (adr, currency) = if let Some(cal) = calendar {
        let prices: Vec<f64> = cal
            .days
            .iter()
            .filter(|d| d.available)
            .filter_map(|d| d.price)
            .collect();
        let avg = if prices.is_empty() {
            neighborhood.and_then(|n| n.average_price).unwrap_or(0.0)
        } else {
            prices.iter().sum::<f64>() / prices.len() as f64
        };
        (avg, cal.currency.clone())
    } else {
        (
            neighborhood.and_then(|n| n.average_price).unwrap_or(0.0),
            "$".to_string(),
        )
    };

    // Occupancy rate: prefer computed occupancy, fallback to 65% industry average
    let occ_rate = occupancy.map_or(65.0, |o| o.occupancy_rate);

    // vs neighborhood
    let vs_neighborhood = neighborhood
        .and_then(|n| n.average_price)
        .filter(|&avg| avg > 0.0)
        .map(|avg| (adr - avg) / avg * 100.0);

    // Monthly breakdown from occupancy if available
    let monthly_breakdown: Vec<MonthlyRevenue> = if let Some(occ) = occupancy {
        occ.monthly_breakdown
            .iter()
            .map(|m| {
                let rate = m.average_price.unwrap_or(adr);
                let occupied = m.occupied_days;
                MonthlyRevenue {
                    month: m.month.clone(),
                    projected_revenue: rate * f64::from(occupied),
                    projected_occupancy_pct: m.occupancy_rate,
                    avg_nightly_rate: rate,
                }
            })
            .collect()
    } else {
        vec![]
    };

    let monthly_revenue = adr * (occ_rate / 100.0) * 30.44; // avg days per month (365.25/12)
    let annual_revenue = monthly_revenue * 12.0;

    RevenueEstimate {
        listing_id: listing_id.map(str::to_string),
        location: location.to_string(),
        projected_adr: adr,
        projected_occupancy_pct: occ_rate,
        projected_monthly_revenue: monthly_revenue,
        projected_annual_revenue: annual_revenue,
        vs_neighborhood_avg_price_pct: vs_neighborhood,
        currency,
        monthly_breakdown,
    }
}

// ---------------------------------------------------------------------------
// Listing Score computation
// ---------------------------------------------------------------------------

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
        score: photo_score,
        details: format!("{photo_count} photos"),
    });

    // Description score
    let desc_len = detail.description.len();
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
        score: desc_score,
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
        score: amenity_score,
        details: format!("{amenity_count} amenities listed"),
    });

    // Reviews score
    let reviews_score = match detail.review_count {
        0 => {
            suggestions.push("New listing — focus on getting your first reviews".to_string());
            0.0
        }
        1..=4 => 25.0,
        5..=19 => 50.0,
        20..=49 => 75.0,
        _ => 100.0,
    };
    let rating_info = detail
        .rating
        .map_or_else(|| "no rating".to_string(), |r| format!("{r:.2} rating"));
    categories.push(CategoryScore {
        category: "Reviews".to_string(),
        score: reviews_score,
        details: format!("{} reviews, {rating_info}", detail.review_count),
    });

    // Host score
    let mut host_score: f64 = 50.0;
    let mut host_details = Vec::new();
    if detail.host_is_superhost == Some(true) {
        host_score += 30.0;
        host_details.push("Superhost");
    }
    if detail.host_response_rate.is_some() {
        host_score += 10.0;
        host_details.push("response rate listed");
    }
    if detail.host_response_time.is_some() {
        host_score += 10.0;
        host_details.push("response time listed");
    }
    host_score = host_score.min(100.0);
    categories.push(CategoryScore {
        category: "Host".to_string(),
        score: host_score,
        details: if host_details.is_empty() {
            "basic profile".to_string()
        } else {
            host_details.join(", ")
        },
    });

    // Pricing score (vs neighborhood)
    let pricing_score = if let Some(stats) = neighborhood
        && let Some(avg) = stats.average_price
        && avg > 0.0
    {
        let ratio = detail.price_per_night / avg;
        let score = match ratio {
            r if r < 0.5 => {
                suggestions.push(
                    "Your price is significantly below market — consider raising it".to_string(),
                );
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
        let _ = ratio; // used in match
        score
    } else {
        50.0 // no data to compare
    };
    categories.push(CategoryScore {
        category: "Pricing".to_string(),
        score: pricing_score,
        details: if let Some(stats) = neighborhood
            && let Some(avg) = stats.average_price
        {
            format!(
                "${:.0}/night (market avg: ${avg:.0})",
                detail.price_per_night
            )
        } else {
            format!("${:.0}/night", detail.price_per_night)
        },
    });

    let overall_score = categories.iter().map(|c| c.score).sum::<f64>() / categories.len() as f64;

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

#[allow(clippy::cast_possible_truncation)]
pub fn compute_amenity_analysis(
    detail: &ListingDetail,
    neighborhood_details: &[ListingDetail],
) -> AmenityAnalysis {
    let listing_amenities: std::collections::HashSet<String> = detail
        .amenities
        .iter()
        .map(|a| normalize_amenity(a))
        .collect();
    let listing_amenity_count = listing_amenities.len() as u32;

    // Count amenity frequency across neighborhood
    let total_neighbors = neighborhood_details.len();
    let neighborhood_avg_amenity_count = if total_neighbors == 0 {
        0.0
    } else {
        neighborhood_details
            .iter()
            .map(|d| d.amenities.len() as f64)
            .sum::<f64>()
            / total_neighbors as f64
    };

    let mut amenity_freq: HashMap<String, u32> = HashMap::new();
    for d in neighborhood_details {
        for a in &d.amenities {
            *amenity_freq.entry(normalize_amenity(a)).or_insert(0) += 1;
        }
    }

    let mut missing_popular = Vec::new();
    let mut present_rare = Vec::new();

    if total_neighbors > 0 {
        for (amenity, count) in &amenity_freq {
            let freq_pct = f64::from(*count) / total_neighbors as f64 * 100.0;
            if !listing_amenities.contains(amenity) && freq_pct >= 50.0 {
                missing_popular.push(AmenityGap {
                    amenity: amenity.clone(),
                    neighborhood_frequency_pct: freq_pct,
                    is_present: false,
                });
            }
            if listing_amenities.contains(amenity) && freq_pct < 30.0 {
                present_rare.push(AmenityGap {
                    amenity: amenity.clone(),
                    neighborhood_frequency_pct: freq_pct,
                    is_present: true,
                });
            }
        }
    }

    // Sort: most common missing first, rarest present first
    missing_popular.sort_by(|a, b| {
        b.neighborhood_frequency_pct
            .partial_cmp(&a.neighborhood_frequency_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    present_rare.sort_by(|a, b| {
        a.neighborhood_frequency_pct
            .partial_cmp(&b.neighborhood_frequency_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let amenity_score_pct = if neighborhood_avg_amenity_count > 0.0 {
        (f64::from(listing_amenity_count) / neighborhood_avg_amenity_count * 100.0).min(200.0)
    } else {
        100.0
    };

    AmenityAnalysis {
        listing_id: detail.id.clone(),
        listing_amenity_count,
        neighborhood_avg_amenity_count,
        missing_popular_amenities: missing_popular,
        present_rare_amenities: present_rare,
        amenity_score_pct,
    }
}

// ---------------------------------------------------------------------------
// Compare Listings computation
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
pub fn compute_compare_listings(
    listings: &[Listing],
    _details: Option<&[ListingDetail]>,
) -> CompareListingsResult {
    let count = listings.len() as u32;

    let mut prices: Vec<f64> = listings
        .iter()
        .map(|l| l.price_per_night)
        .filter(|&p| p > 0.0)
        .collect();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let avg_price = if prices.is_empty() {
        0.0
    } else {
        prices.iter().sum::<f64>() / prices.len() as f64
    };
    let median_price = if prices.is_empty() {
        0.0
    } else {
        let mid = prices.len() / 2;
        if prices.len().is_multiple_of(2) {
            f64::midpoint(prices[mid - 1], prices[mid])
        } else {
            prices[mid]
        }
    };
    let price_range = if prices.is_empty() {
        (0.0, 0.0)
    } else {
        (prices[0], prices[prices.len() - 1])
    };

    let ratings: Vec<f64> = listings.iter().filter_map(|l| l.rating).collect();
    let mut sorted_ratings = ratings.clone();
    sorted_ratings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let avg_rating = if ratings.is_empty() {
        None
    } else {
        Some(ratings.iter().sum::<f64>() / ratings.len() as f64)
    };

    let superhost_count = listings
        .iter()
        .filter(|l| l.is_superhost == Some(true))
        .count() as u32;

    let comparisons: Vec<ListingComparison> = listings
        .iter()
        .map(|l| {
            // Price percentile: what % of listings are cheaper
            let price_pct = if prices.len() <= 1 {
                50.0
            } else {
                let pos = prices
                    .iter()
                    .position(|&p| p >= l.price_per_night)
                    .unwrap_or(prices.len());
                pos as f64 / (prices.len() - 1) as f64 * 100.0
            };

            let rating_pct = l.rating.map(|r| {
                if sorted_ratings.len() <= 1 {
                    50.0
                } else {
                    let pos = sorted_ratings
                        .iter()
                        .position(|&sr| sr >= r)
                        .unwrap_or(sorted_ratings.len());
                    pos as f64 / (sorted_ratings.len() - 1) as f64 * 100.0
                }
            });

            ListingComparison {
                id: l.id.clone(),
                name: l.name.clone(),
                price_per_night: l.price_per_night,
                currency: l.currency.clone(),
                rating: l.rating,
                review_count: l.review_count,
                property_type: l.property_type.clone(),
                is_superhost: l.is_superhost,
                bedrooms: None, // Not available from Listing, only from ListingDetail
                amenities_count: None,
                price_percentile: price_pct,
                rating_percentile: rating_pct,
            }
        })
        .collect();

    CompareListingsResult {
        listings: comparisons,
        summary: ComparisonSummary {
            count,
            avg_price,
            median_price,
            avg_rating,
            price_range,
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
        })
        .collect();

    MarketComparison { locations }
}

// ---------------------------------------------------------------------------
// Host Portfolio computation
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
pub fn compute_host_portfolio(
    host_name: &str,
    host_id: Option<&str>,
    is_superhost: Option<bool>,
    listings: &[Listing],
) -> HostPortfolio {
    let total_properties = listings.len() as u32;

    let prices: Vec<f64> = listings
        .iter()
        .map(|l| l.price_per_night)
        .filter(|&p| p > 0.0)
        .collect();
    let avg_price = if prices.is_empty() {
        0.0
    } else {
        prices.iter().sum::<f64>() / prices.len() as f64
    };
    let price_range = if prices.is_empty() {
        (0.0, 0.0)
    } else {
        let min = prices.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let max = prices.iter().copied().reduce(f64::max).unwrap_or(0.0);
        (min, max)
    };

    let ratings: Vec<f64> = listings.iter().filter_map(|l| l.rating).collect();
    let avg_rating = if ratings.is_empty() {
        None
    } else {
        Some(ratings.iter().sum::<f64>() / ratings.len() as f64)
    };

    let total_reviews: u32 = listings.iter().map(|l| l.review_count).sum();

    let properties: Vec<PortfolioProperty> = listings
        .iter()
        .map(|l| PortfolioProperty {
            id: l.id.clone(),
            name: l.name.clone(),
            location: l.location.clone(),
            price_per_night: l.price_per_night,
            rating: l.rating,
            review_count: l.review_count,
            property_type: l.property_type.clone(),
        })
        .collect();

    HostPortfolio {
        host_name: host_name.to_string(),
        host_id: host_id.map(str::to_string),
        total_properties,
        avg_rating,
        avg_price,
        price_range,
        total_reviews,
        is_superhost,
        properties,
    }
}

// ---------------------------------------------------------------------------
// Review Sentiment computation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn compute_review_sentiment(listing_id: &str, reviews: &[Review]) -> ReviewSentiment {
    let total = reviews.len() as u32;

    let positive_keywords: &[&str] = &[
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
    let negative_keywords: &[&str] = &[
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

    let theme_map: &[(&str, &[&str])] = &[
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

    let mut positive_count = 0u32;
    let mut negative_count = 0u32;
    let mut neutral_count = 0u32;

    let mut pos_keyword_counts: HashMap<String, u32> = HashMap::new();
    let mut neg_keyword_counts: HashMap<String, u32> = HashMap::new();

    // Per-theme tracking: (mention_count, positive_count, negative_count, sample_quotes)
    let mut theme_data: HashMap<&str, (u32, u32, u32, Vec<String>)> = HashMap::new();
    for &(theme_name, _) in theme_map {
        theme_data.insert(theme_name, (0, 0, 0, Vec::new()));
    }

    for review in reviews {
        let comment_lower = review.comment.to_lowercase();

        let mut pos_hits = 0u32;
        let mut neg_hits = 0u32;

        for &kw in positive_keywords {
            let count = comment_lower.matches(kw).count() as u32;
            if count > 0 {
                pos_hits += count;
                *pos_keyword_counts.entry(kw.to_string()).or_insert(0) += count;
            }
        }
        for &kw in negative_keywords {
            let count = comment_lower.matches(kw).count() as u32;
            if count > 0 {
                neg_hits += count;
                *neg_keyword_counts.entry(kw.to_string()).or_insert(0) += count;
            }
        }

        match pos_hits.cmp(&neg_hits) {
            std::cmp::Ordering::Greater => positive_count += 1,
            std::cmp::Ordering::Less => negative_count += 1,
            std::cmp::Ordering::Equal => neutral_count += 1,
        }

        // Theme analysis
        let is_positive_review = pos_hits > neg_hits;
        let is_negative_review = neg_hits > pos_hits;

        for &(theme_name, theme_keywords) in theme_map {
            let has_theme_keyword = theme_keywords.iter().any(|&kw| comment_lower.contains(kw));
            if has_theme_keyword {
                let entry = theme_data.get_mut(theme_name).unwrap();
                entry.0 += 1; // mention_count
                if is_positive_review {
                    entry.1 += 1; // positive_count
                }
                if is_negative_review {
                    entry.2 += 1; // negative_count
                }
                if entry.3.len() < 2 {
                    let truncated: String = review.comment.chars().take(100).collect();
                    entry.3.push(truncated);
                }
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

    let mut themes: Vec<ReviewTheme> = theme_data
        .into_iter()
        .filter(|(_, (mention_count, _, _, _))| *mention_count > 0)
        .map(
            |(theme_name, (mention_count, pos_count, neg_count, quotes))| ReviewTheme {
                theme: theme_name.to_string(),
                mention_count,
                positive_count: pos_count,
                negative_count: neg_count,
                sample_quotes: quotes,
            },
        )
        .collect();
    themes.sort_by(|a, b| b.mention_count.cmp(&a.mention_count));

    let mut top_positive: Vec<(String, u32)> = pos_keyword_counts.into_iter().collect();
    top_positive.sort_by(|a, b| b.1.cmp(&a.1));
    top_positive.truncate(10);

    let mut top_negative: Vec<(String, u32)> = neg_keyword_counts.into_iter().collect();
    top_negative.sort_by(|a, b| b.1.cmp(&a.1));
    top_negative.truncate(10);

    ReviewSentiment {
        listing_id: listing_id.to_string(),
        total_reviews_analyzed: total,
        positive_pct,
        negative_pct,
        neutral_pct,
        themes,
        top_positive_keywords: top_positive,
        top_negative_keywords: top_negative,
    }
}

// ---------------------------------------------------------------------------
// Competitive Positioning computation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
pub fn compute_competitive_positioning(
    detail: &ListingDetail,
    neighborhood: &NeighborhoodStats,
    occupancy: Option<&OccupancyEstimate>,
    amenity_analysis: Option<&AmenityAnalysis>,
) -> CompetitivePositioning {
    let mut axes = Vec::new();

    // 1. Price Value (lower price relative to market = better value = higher score)
    let price_axis = if let Some(median) = neighborhood.median_price
        && median > 0.0
    {
        let ratio = detail.price_per_night / median * 100.0;
        let percentile = (200.0 - ratio).clamp(0.0, 100.0);
        let assessment = if percentile >= 70.0 {
            "Strong value".to_string()
        } else if percentile >= 40.0 {
            "Fair value".to_string()
        } else {
            "Premium priced".to_string()
        };
        CompetitiveAxis {
            axis: "Price Value".to_string(),
            listing_value: detail.price_per_night,
            neighborhood_avg: median,
            percentile,
            assessment,
        }
    } else {
        CompetitiveAxis {
            axis: "Price Value".to_string(),
            listing_value: detail.price_per_night,
            neighborhood_avg: 0.0,
            percentile: 50.0,
            assessment: "No market data".to_string(),
        }
    };
    axes.push(price_axis);

    // 2. Rating
    let rating_axis = if let Some(rating) = detail.rating
        && let Some(avg_rating) = neighborhood.average_rating
        && avg_rating > 0.0
    {
        // Scale percentile: 5.0 is max, position relative to avg
        let diff = rating - avg_rating;
        let percentile = (50.0 + diff * 50.0).clamp(0.0, 100.0);
        let assessment = if percentile >= 70.0 {
            "Above average".to_string()
        } else if percentile >= 40.0 {
            "Average".to_string()
        } else {
            "Below average".to_string()
        };
        CompetitiveAxis {
            axis: "Rating".to_string(),
            listing_value: rating,
            neighborhood_avg: avg_rating,
            percentile,
            assessment,
        }
    } else {
        CompetitiveAxis {
            axis: "Rating".to_string(),
            listing_value: detail.rating.unwrap_or(0.0),
            neighborhood_avg: neighborhood.average_rating.unwrap_or(0.0),
            percentile: 50.0,
            assessment: "Insufficient data".to_string(),
        }
    };
    axes.push(rating_axis);

    // 3. Amenity Count
    let amenity_axis = if let Some(aa) = amenity_analysis {
        let percentile = aa.amenity_score_pct.clamp(0.0, 100.0);
        let assessment = if percentile >= 70.0 {
            "Well equipped".to_string()
        } else if percentile >= 40.0 {
            "Adequate".to_string()
        } else {
            "Under-equipped".to_string()
        };
        CompetitiveAxis {
            axis: "Amenity Count".to_string(),
            listing_value: f64::from(aa.listing_amenity_count),
            neighborhood_avg: aa.neighborhood_avg_amenity_count,
            percentile,
            assessment,
        }
    } else {
        CompetitiveAxis {
            axis: "Amenity Count".to_string(),
            listing_value: detail.amenities.len() as f64,
            neighborhood_avg: 0.0,
            percentile: 50.0,
            assessment: "No comparison data".to_string(),
        }
    };
    axes.push(amenity_axis);

    // 4. Review Volume (benchmark: 50 reviews = 100%)
    let review_percentile = (f64::from(detail.review_count) / 50.0 * 100.0).clamp(0.0, 100.0);
    let review_assessment = if review_percentile >= 70.0 {
        "Well reviewed".to_string()
    } else if review_percentile >= 40.0 {
        "Moderate reviews".to_string()
    } else {
        "Few reviews".to_string()
    };
    axes.push(CompetitiveAxis {
        axis: "Review Volume".to_string(),
        listing_value: f64::from(detail.review_count),
        neighborhood_avg: 50.0,
        percentile: review_percentile,
        assessment: review_assessment,
    });

    // 5. Occupancy
    let occupancy_axis = if let Some(occ) = occupancy {
        let percentile = occ.occupancy_rate.clamp(0.0, 100.0);
        let assessment = if percentile >= 70.0 {
            "High demand".to_string()
        } else if percentile >= 40.0 {
            "Moderate demand".to_string()
        } else {
            "Low demand".to_string()
        };
        CompetitiveAxis {
            axis: "Occupancy".to_string(),
            listing_value: occ.occupancy_rate,
            neighborhood_avg: 65.0, // industry average
            percentile,
            assessment,
        }
    } else {
        CompetitiveAxis {
            axis: "Occupancy".to_string(),
            listing_value: 0.0,
            neighborhood_avg: 65.0,
            percentile: 50.0,
            assessment: "No occupancy data".to_string(),
        }
    };
    axes.push(occupancy_axis);

    let overall_competitiveness =
        axes.iter().map(|a| a.percentile).sum::<f64>() / axes.len() as f64;

    let strengths: Vec<String> = axes
        .iter()
        .filter(|a| a.percentile >= 70.0)
        .map(|a| a.axis.clone())
        .collect();

    let weaknesses: Vec<String> = axes
        .iter()
        .filter(|a| a.percentile <= 30.0)
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

pub fn compute_optimal_pricing(
    detail: &ListingDetail,
    neighborhood: Option<&NeighborhoodStats>,
    price_trends: Option<&PriceTrends>,
    amenity_analysis: Option<&AmenityAnalysis>,
) -> PricingRecommendation {
    let current_price = detail.price_per_night;
    let currency = detail.currency.clone();
    let mut reasoning = Vec::new();

    // Start with neighborhood median as baseline, or current price if unavailable
    let baseline = if let Some(stats) = neighborhood
        && let Some(median) = stats.median_price
        && median > 0.0
    {
        reasoning.push(format!("Baseline: neighborhood median ${median:.2}/night"));
        median
    } else {
        reasoning.push(format!(
            "Baseline: current listing price ${current_price:.2}/night (no neighborhood data)"
        ));
        current_price
    };

    let mut recommended = baseline;

    // Adjust for rating
    let rating_adjustment = if let Some(rating) = detail.rating
        && let Some(stats) = neighborhood
        && let Some(avg_rating) = stats.average_rating
        && avg_rating > 0.0
    {
        let diff = rating - avg_rating;
        // Each 0.1 rating point above average = ~2% premium
        let pct = diff * 20.0;
        let adj = baseline * pct / 100.0;
        recommended += adj;
        reasoning.push(format!(
            "Rating adjustment: {pct:+.1}% (your {rating:.2} vs avg {avg_rating:.2})"
        ));
        adj
    } else {
        0.0
    };
    let _ = rating_adjustment;

    // Adjust for amenities
    let amenity_premium_pct = if let Some(aa) = amenity_analysis {
        let score = aa.amenity_score_pct;
        if (score - 100.0).abs() > f64::EPSILON {
            let pct = (score - 100.0) * 0.1; // 10% of the amenity difference
            let adj = baseline * pct / 100.0;
            recommended += adj;
            reasoning.push(format!(
                "Amenity adjustment: {pct:+.1}% (amenity score {score:.0}% vs market)"
            ));
            Some(pct)
        } else {
            reasoning.push("Amenities in line with market".to_string());
            Some(0.0)
        }
    } else {
        None
    };

    // Ensure recommended price is positive
    recommended = recommended.max(1.0);

    // Compute range as +/-15%
    let range_low = recommended * 0.85;
    let range_high = recommended * 1.15;

    // Weekend / weekday split
    let (weekday_rec, weekend_rec) = if let Some(trends) = price_trends
        && let Some(premium_pct) = trends.weekend_premium_pct
    {
        // Split so that the average comes out to recommended
        // weekend = recommended * (1 + premium/200), weekday = recommended * (1 - premium/200)
        let factor = premium_pct / 200.0;
        let weekday = recommended * (1.0 - factor);
        let weekend = recommended * (1.0 + factor);
        reasoning.push(format!(
            "Weekend premium: {premium_pct:+.1}% applied to weekday/weekend split"
        ));
        (Some(weekday), Some(weekend))
    } else {
        (None, None)
    };

    // vs neighborhood median
    let vs_median = if let Some(stats) = neighborhood
        && let Some(median) = stats.median_price
        && median > 0.0
    {
        Some((recommended - median) / median * 100.0)
    } else {
        None
    };

    PricingRecommendation {
        listing_id: detail.id.clone(),
        current_price,
        recommended_price: recommended,
        recommended_range: (range_low, range_high),
        currency,
        reasoning,
        weekday_recommendation: weekday_rec,
        weekend_recommendation: weekend_rec,
        amenity_premium_pct,
        vs_neighborhood_median: vs_median,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{
        make_calendar_day, make_listing, make_listing_detail, make_price_calendar,
    };

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
        assert!((trends.overall_avg - 165.0).abs() < 0.01);
        assert!((trends.overall_min - 100.0).abs() < 0.01);
        assert!((trends.overall_max - 250.0).abs() < 0.01);
        assert!(trends.price_volatility > 0.0);
        assert!(trends.weekend_premium_pct.is_some());
        // Weekend avg (200+250)/2=225, weekday avg (100+110)/2=105
        // Premium = (225-105)/105*100 = 114.3%
        assert!((trends.weekend_premium_pct.unwrap() - 114.28).abs() < 1.0);
    }

    #[test]
    fn price_trends_empty_calendar() {
        let cal = make_price_calendar("42", vec![]);
        let trends = compute_price_trends("42", &cal);

        assert!((trends.overall_avg - 0.0).abs() < 0.01);
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
        assert!((trends.monthly[0].avg_price - 110.0).abs() < 0.01);
        assert_eq!(trends.monthly[1].month, "2025-07");
        assert!((trends.monthly[1].avg_price - 210.0).abs() < 0.01);
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
        assert_eq!(result.suggested_min_nights, Some(2));
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
        let occ = compute_occupancy_estimate("42", &cal);
        let est = compute_revenue_estimate(Some("42"), "Paris", Some(&cal), None, Some(&occ));

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
        };
        let est = compute_revenue_estimate(None, "Paris", None, Some(&stats), None);

        assert!((est.projected_adr - 120.0).abs() < 0.01);
        assert!((est.projected_occupancy_pct - 65.0).abs() < 0.01); // industry default
    }

    #[test]
    fn revenue_estimate_display() {
        let est = RevenueEstimate {
            listing_id: Some("42".to_string()),
            location: "Paris".to_string(),
            projected_adr: 150.0,
            projected_occupancy_pct: 70.0,
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
        };
        let score = compute_listing_score(&detail, Some(&stats));

        let pricing_cat = score
            .category_scores
            .iter()
            .find(|c| c.category == "Pricing")
            .unwrap();
        assert!((pricing_cat.score - 100.0).abs() < 0.01); // price matches market
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
        assert!((result.summary.avg_price - 116.66).abs() < 1.0);
        assert!((result.summary.median_price - 100.0).abs() < 0.01);
        assert_eq!(result.summary.price_range, (50.0, 200.0));
        assert_eq!(result.listings.len(), 3);
    }

    #[test]
    fn compare_listings_single() {
        let listings = vec![make_listing("1", "Solo", 100.0)];
        let result = compute_compare_listings(&listings, None);
        assert_eq!(result.summary.count, 1);
        assert!((result.listings[0].price_percentile - 50.0).abs() < 0.01);
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

    #[test]
    fn host_portfolio_basic() {
        let listings = vec![
            make_listing("1", "Apt 1", 100.0),
            make_listing("2", "Apt 2", 200.0),
        ];
        let result = compute_host_portfolio("Alice", Some("123"), Some(true), &listings);

        assert_eq!(result.host_name, "Alice");
        assert_eq!(result.total_properties, 2);
        assert!((result.avg_price - 150.0).abs() < 0.01);
        assert_eq!(result.price_range, (100.0, 200.0));
        assert_eq!(result.total_reviews, 20); // 10 each from factory
        assert_eq!(result.is_superhost, Some(true));
    }

    #[test]
    fn host_portfolio_empty() {
        let result = compute_host_portfolio("Bob", None, None, &[]);
        assert_eq!(result.total_properties, 0);
        assert!((result.avg_price - 0.0).abs() < 0.01);
        assert!(result.avg_rating.is_none());
    }

    #[test]
    fn host_portfolio_display() {
        let listings = vec![make_listing("1", "Apt", 100.0)];
        let result = compute_host_portfolio("Alice", Some("123"), Some(true), &listings);
        let s = result.to_string();
        assert!(s.contains("Host Portfolio: Alice"));
        assert!(s.contains("Superhost: Yes"));
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
    // Competitive Positioning tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_competitive_positioning_basic() {
        let detail = make_listing_detail("42");
        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(100.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.5),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
        };
        let result = compute_competitive_positioning(&detail, &stats, None, None);

        assert_eq!(result.listing_id, "42");
        assert_eq!(result.axes.len(), 5);
        assert!(result.overall_competitiveness > 0.0);
        assert!(result.overall_competitiveness <= 100.0);
        // The listing has price 100 and median is 100 -> ratio 100%, percentile = 200-100 = 100
        let price_axis = result
            .axes
            .iter()
            .find(|a| a.axis == "Price Value")
            .unwrap();
        assert!((price_axis.percentile - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_competitive_positioning_no_neighborhood() {
        let detail = make_listing_detail("42");
        let stats = NeighborhoodStats {
            location: "Unknown".to_string(),
            total_listings: 0,
            average_price: None,
            median_price: None,
            price_range: None,
            average_rating: None,
            property_type_distribution: vec![],
            superhost_percentage: None,
        };
        let result = compute_competitive_positioning(&detail, &stats, None, None);

        assert_eq!(result.axes.len(), 5);
        // All axes should default to 50 percentile when no data
        let price_axis = result
            .axes
            .iter()
            .find(|a| a.axis == "Price Value")
            .unwrap();
        assert!((price_axis.percentile - 50.0).abs() < 0.01);
        let rating_axis = result.axes.iter().find(|a| a.axis == "Rating").unwrap();
        assert!((rating_axis.percentile - 50.0).abs() < 0.01);
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
        };
        let days = vec![
            make_calendar_day("2025-06-06", Some(200.0), true), // Fri
            make_calendar_day("2025-06-07", Some(250.0), true), // Sat
            make_calendar_day("2025-06-09", Some(100.0), true), // Mon
            make_calendar_day("2025-06-10", Some(110.0), true), // Tue
        ];
        let cal = make_price_calendar("42", days);
        let trends = compute_price_trends("42", &cal);

        let rec = compute_optimal_pricing(&detail, Some(&stats), Some(&trends), None);

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
        assert_eq!(result.summary.price_range, (80.0, 250.0));
        assert!(result.summary.avg_rating.is_some());

        // Verify both listings appear by name
        let names: Vec<&str> = result.listings.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"Budget Apt"));
        assert!(names.contains(&"Luxury Suite"));

        // Verify percentile ordering: cheaper listing should have lower price percentile
        let budget = result.listings.iter().find(|l| l.id == "1").unwrap();
        let luxury = result.listings.iter().find(|l| l.id == "2").unwrap();
        assert!(budget.price_percentile < luxury.price_percentile);

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
        };

        let score = compute_listing_score(&detail, Some(&stats));
        assert!(
            score.overall_score > 80.0,
            "Perfect listing should score > 80, got {}",
            score.overall_score
        );
        // Check each category is high
        for cat in &score.category_scores {
            assert!(
                cat.score >= 75.0,
                "Category {} should score >= 75, got {}",
                cat.category,
                cat.score
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
    // Competitive Positioning deeper tests
    // -----------------------------------------------------------------------

    #[test]
    fn competitive_positioning_all_axes() {
        let mut detail = make_listing_detail("42");
        detail.review_count = 60; // above the 50 benchmark -> high percentile
        detail.rating = Some(4.9);

        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(120.0),
            median_price: Some(110.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.3),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
        };

        let occupancy = OccupancyEstimate {
            listing_id: "42".to_string(),
            period_start: "2025-06-01".to_string(),
            period_end: "2025-08-31".to_string(),
            total_days: 90,
            occupied_days: 72,
            available_days: 18,
            occupancy_rate: 80.0,
            average_available_price: Some(110.0),
            weekend_avg_price: Some(130.0),
            weekday_avg_price: Some(100.0),
            monthly_breakdown: vec![],
        };

        let amenity_analysis = AmenityAnalysis {
            listing_id: "42".to_string(),
            listing_amenity_count: 15,
            neighborhood_avg_amenity_count: 10.0,
            missing_popular_amenities: vec![],
            present_rare_amenities: vec![],
            amenity_score_pct: 150.0, // 15/10 * 100 = 150, clamped to 100 in percentile
        };

        let result = compute_competitive_positioning(
            &detail,
            &stats,
            Some(&occupancy),
            Some(&amenity_analysis),
        );

        assert_eq!(result.axes.len(), 5);

        // Verify all 5 axes present
        let axis_names: Vec<&str> = result.axes.iter().map(|a| a.axis.as_str()).collect();
        assert!(axis_names.contains(&"Price Value"));
        assert!(axis_names.contains(&"Rating"));
        assert!(axis_names.contains(&"Amenity Count"));
        assert!(axis_names.contains(&"Review Volume"));
        assert!(axis_names.contains(&"Occupancy"));

        // Review Volume: 60 reviews / 50 benchmark * 100 = 120, clamped to 100
        let review_axis = result
            .axes
            .iter()
            .find(|a| a.axis == "Review Volume")
            .unwrap();
        assert!(
            (review_axis.percentile - 100.0).abs() < 0.01,
            "Review volume should be at 100 percentile, got {}",
            review_axis.percentile
        );

        // Occupancy: 80% occupancy rate -> percentile 80
        let occ_axis = result.axes.iter().find(|a| a.axis == "Occupancy").unwrap();
        assert!(
            (occ_axis.percentile - 80.0).abs() < 0.01,
            "Occupancy should be at 80 percentile, got {}",
            occ_axis.percentile
        );
    }

    #[test]
    fn competitive_positioning_strengths_weaknesses() {
        let mut detail = make_listing_detail("42");
        detail.price_per_night = 200.0; // expensive relative to median
        detail.rating = Some(4.9); // high rating
        detail.review_count = 5; // few reviews -> low percentile

        let stats = NeighborhoodStats {
            location: "Paris".to_string(),
            total_listings: 100,
            average_price: Some(100.0),
            median_price: Some(100.0),
            price_range: Some((50.0, 300.0)),
            average_rating: Some(4.3),
            property_type_distribution: vec![],
            superhost_percentage: Some(30.0),
        };

        let result = compute_competitive_positioning(&detail, &stats, None, None);

        // Rating: 4.9 vs avg 4.3 -> diff 0.6 -> percentile = 50 + 0.6*50 = 80 -> strength
        assert!(
            result.strengths.contains(&"Rating".to_string()),
            "Rating should be a strength, strengths: {:?}",
            result.strengths
        );

        // Review Volume: 5/50*100 = 10 -> weakness (<=30)
        assert!(
            result.weaknesses.contains(&"Review Volume".to_string()),
            "Review Volume should be a weakness, weaknesses: {:?}",
            result.weaknesses
        );

        // Price Value: price 200 / median 100 = 200%, percentile = 200-200 = 0 -> weakness
        assert!(
            result.weaknesses.contains(&"Price Value".to_string()),
            "Price Value should be a weakness, weaknesses: {:?}",
            result.weaknesses
        );
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

        let rec = compute_optimal_pricing(&detail, Some(&stats), Some(&trends), None);

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

        let rec = compute_optimal_pricing(&detail, None, None, None);

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
        };

        let rec = compute_optimal_pricing(&detail, Some(&stats), None, None);

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
}
