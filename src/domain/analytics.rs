#![allow(clippy::cast_precision_loss)] // Counts are small enough for f64

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use super::calendar::PriceCalendar;
use super::listing::Listing;

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

// ---------------------------------------------------------------------------
// Pure computation functions
// ---------------------------------------------------------------------------

pub fn compute_neighborhood_stats(location: &str, listings: &[Listing]) -> NeighborhoodStats {
    let total_listings = listings.len() as u32;

    // Prices
    let mut prices: Vec<f64> = listings.iter().map(|l| l.price_per_night).collect();
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_calendar_day, make_listing, make_price_calendar};

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
}
