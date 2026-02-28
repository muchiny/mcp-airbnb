#![allow(clippy::cast_precision_loss)]

use serde::{Deserialize, Serialize};

/// Reason why a calendar day is unavailable.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub enum UnavailabilityReason {
    /// Reason could not be determined from available data.
    Unknown,
    /// The day is booked by a guest (reservation exists).
    Booked,
    /// The host has manually blocked this date.
    BlockedByHost,
    /// The date is in the past and therefore unavailable.
    PastDate,
    /// Unavailable due to minimum night stay restriction.
    MinNightRestriction,
}

impl std::fmt::Display for UnavailabilityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::Booked => write!(f, "Booked"),
            Self::BlockedByHost => write!(f, "Blocked by host"),
            Self::PastDate => write!(f, "Past date"),
            Self::MinNightRestriction => write!(f, "Min night restriction"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarDay {
    pub date: String,
    pub price: Option<f64>,
    pub available: bool,
    pub min_nights: Option<u32>,
    #[serde(default)]
    pub max_nights: Option<u32>,
    #[serde(default)]
    pub closed_to_arrival: Option<bool>,
    #[serde(default)]
    pub closed_to_departure: Option<bool>,
    #[serde(default)]
    pub unavailability_reason: Option<UnavailabilityReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceCalendar {
    pub listing_id: String,
    pub currency: String,
    pub days: Vec<CalendarDay>,
    #[serde(default)]
    pub average_price: Option<f64>,
    #[serde(default)]
    pub occupancy_rate: Option<f64>,
    #[serde(default)]
    pub min_price: Option<f64>,
    #[serde(default)]
    pub max_price: Option<f64>,
}

impl PriceCalendar {
    /// Compute summary statistics from the day-by-day data.
    pub fn compute_stats(&mut self) {
        let prices: Vec<f64> = self
            .days
            .iter()
            .filter(|d| d.available)
            .filter_map(|d| d.price)
            .collect();
        if !prices.is_empty() {
            self.average_price = Some(prices.iter().sum::<f64>() / prices.len() as f64);
            self.min_price = prices.iter().copied().reduce(f64::min);
            self.max_price = prices.iter().copied().reduce(f64::max);
        }
        let total = self.days.len();
        if total > 0 {
            let unavailable = self.days.iter().filter(|d| !d.available).count();
            self.occupancy_rate = Some(unavailable as f64 / total as f64 * 100.0);
        }
    }
}

impl std::fmt::Display for PriceCalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Price calendar for listing {} ({})",
            self.listing_id, self.currency
        )?;
        writeln!(
            f,
            "{:<12} {:>8} {:>10} {:>10}",
            "Date", "Price", "Available", "Min nights"
        )?;
        if let Some(occ) = self.occupancy_rate {
            writeln!(f, "Occupancy: {occ:.1}%")?;
        }
        if let Some(avg) = self.average_price {
            write!(f, "Avg price: {}{avg:.0}", self.currency)?;
            if let (Some(min), Some(max)) = (self.min_price, self.max_price) {
                write!(
                    f,
                    " (range: {}{min:.0}-{}{max:.0})",
                    self.currency, self.currency
                )?;
            }
            writeln!(f)?;
        }
        writeln!(f, "{}", "-".repeat(44))?;
        for day in &self.days {
            let price = day
                .price
                .map_or_else(|| "-".to_string(), |p| format!("{}{p:.0}", self.currency));
            let available = if day.available {
                "Yes".to_string()
            } else if let Some(reason) = &day.unavailability_reason {
                format!("No ({reason})")
            } else {
                "No".to_string()
            };
            let min_nights = day
                .min_nights
                .map_or_else(|| "-".to_string(), |n| n.to_string());
            writeln!(
                f,
                "{:<12} {:>8} {:>10} {:>10}",
                day.date, price, available, min_nights
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_display_header() {
        let cal = PriceCalendar {
            listing_id: "42".into(),
            currency: "EUR".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: Some(100.0),
                available: true,
                min_nights: None,
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let s = cal.to_string();
        assert!(s.contains("listing 42"));
        assert!(s.contains("(EUR)"));
    }

    #[test]
    fn calendar_display_available_day() {
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: Some(150.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let s = cal.to_string();
        assert!(s.contains("Yes"));
        assert!(s.contains("$150"));
    }

    #[test]
    fn calendar_display_unavailable_day() {
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: Some(100.0),
                available: false,
                min_nights: None,
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let s = cal.to_string();
        assert!(s.contains("No"));
    }

    #[test]
    fn compute_stats_basic() {
        let mut cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![
                CalendarDay {
                    date: "2025-06-01".into(),
                    price: Some(100.0),
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-02".into(),
                    price: Some(200.0),
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-03".into(),
                    price: Some(150.0),
                    available: false,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
            ],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        assert!((cal.average_price.unwrap() - 150.0).abs() < 0.01);
        assert!((cal.min_price.unwrap() - 100.0).abs() < 0.01);
        assert!((cal.max_price.unwrap() - 200.0).abs() < 0.01);
        // 1 out of 3 is unavailable => 33.3%
        assert!((cal.occupancy_rate.unwrap() - 33.333).abs() < 1.0);
    }

    #[test]
    fn compute_stats_empty_days() {
        let mut cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        assert!(cal.average_price.is_none());
        assert!(cal.min_price.is_none());
        assert!(cal.max_price.is_none());
        assert!(cal.occupancy_rate.is_none());
    }

    #[test]
    fn compute_stats_all_unavailable() {
        let mut cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![
                CalendarDay {
                    date: "2025-06-01".into(),
                    price: Some(100.0),
                    available: false,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-02".into(),
                    price: Some(120.0),
                    available: false,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
            ],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        // No available days with prices => average_price stays None
        assert!(cal.average_price.is_none());
        // All unavailable => 100% occupancy
        assert!((cal.occupancy_rate.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn compute_stats_no_prices() {
        let mut cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![
                CalendarDay {
                    date: "2025-06-01".into(),
                    price: None,
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-02".into(),
                    price: None,
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
            ],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        assert!(cal.average_price.is_none());
        assert!(cal.min_price.is_none());
        assert!((cal.occupancy_rate.unwrap() - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_stats_mixed() {
        let mut cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![
                CalendarDay {
                    date: "2025-06-01".into(),
                    price: Some(100.0),
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-02".into(),
                    price: None,
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-03".into(),
                    price: Some(200.0),
                    available: false,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
                CalendarDay {
                    date: "2025-06-04".into(),
                    price: Some(150.0),
                    available: true,
                    min_nights: None,
                    max_nights: None,
                    closed_to_arrival: None,
                    closed_to_departure: None,
                    unavailability_reason: None,
                },
            ],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        // Only available days with prices: 100 and 150 => avg = 125
        assert!((cal.average_price.unwrap() - 125.0).abs() < 0.01);
        assert!((cal.min_price.unwrap() - 100.0).abs() < 0.01);
        assert!((cal.max_price.unwrap() - 150.0).abs() < 0.01);
        // 1 out of 4 unavailable => 25%
        assert!((cal.occupancy_rate.unwrap() - 25.0).abs() < 0.01);
    }

    #[test]
    fn calendar_display_missing_fields() {
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: None,
                available: false,
                min_nights: None,
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let s = cal.to_string();
        // Missing price and min_nights should show "-"
        let lines: Vec<&str> = s.lines().collect();
        let day_line = lines.iter().find(|l| l.contains("2025-06-01")).unwrap();
        // Date "2025-06-01" has 2 hyphens, plus 2 placeholder "-" for price and min_nights = 4
        assert_eq!(day_line.matches('-').count(), 4);
    }

    #[test]
    fn unavailability_reason_display_all_variants() {
        assert_eq!(UnavailabilityReason::Unknown.to_string(), "Unknown");
        assert_eq!(UnavailabilityReason::Booked.to_string(), "Booked");
        assert_eq!(
            UnavailabilityReason::BlockedByHost.to_string(),
            "Blocked by host"
        );
        assert_eq!(UnavailabilityReason::PastDate.to_string(), "Past date");
        assert_eq!(
            UnavailabilityReason::MinNightRestriction.to_string(),
            "Min night restriction"
        );
    }

    #[test]
    fn calendar_display_with_unavailability_reason() {
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![CalendarDay {
                date: "2025-06-01".into(),
                price: Some(120.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: Some(UnavailabilityReason::Booked),
            }],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let s = cal.to_string();
        assert!(
            s.contains("(Booked)"),
            "Display should contain '(Booked)', got: {s}"
        );
    }
}
