use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::num::count_f64;

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

impl CalendarDay {
    /// The day's date when it is a valid `YYYY-MM-DD` string.
    ///
    /// Analytics use this instead of slicing the raw string, so a malformed
    /// or non-ASCII date from upstream is skipped instead of panicking (the
    /// release profile aborts on panic).
    pub fn parsed_date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok()
    }
}

/// How a calendar day counts in occupancy-style analytics (shared by the
/// calendar stats, occupancy, gaps, trends and revenue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayStatus {
    /// Before the reference date. Excluded from every rate.
    Past,
    /// Open for booking.
    Open,
    /// Unavailable because it is booked or for a reason Airbnb does not
    /// disclose. Airbnb does not tell bookings apart from host blocks, so
    /// counting these nights as occupied gives an upper bound.
    Occupied,
    /// Explicitly blocked by the host or closed by a stay rule. Excluded
    /// from occupancy rates.
    Blocked,
}

impl CalendarDay {
    /// Classify the day for occupancy analytics from `available` and
    /// `unavailability_reason`.
    pub fn status(&self) -> DayStatus {
        if self.available {
            return DayStatus::Open;
        }
        match self.unavailability_reason {
            Some(UnavailabilityReason::PastDate) => DayStatus::Past,
            Some(
                UnavailabilityReason::BlockedByHost | UnavailabilityReason::MinNightRestriction,
            ) => DayStatus::Blocked,
            Some(UnavailabilityReason::Booked | UnavailabilityReason::Unknown) | None => {
                DayStatus::Occupied
            }
        }
    }
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
    ///
    /// Prices come from open days that have a known positive price.
    /// Occupancy is the share of occupied nights among open + occupied
    /// nights. Past and blocked days are excluded, and days with an
    /// unparseable date are ignored. Every field is reset, so calling this
    /// again after relabelling days is safe.
    pub fn compute_stats(&mut self) {
        let dated: Vec<&CalendarDay> = self
            .days
            .iter()
            .filter(|d| d.parsed_date().is_some())
            .collect();
        let prices: Vec<f64> = dated
            .iter()
            .filter(|d| d.status() == DayStatus::Open)
            .filter_map(|d| d.price)
            .filter(|p| p.is_finite() && *p > 0.0)
            .collect();
        if prices.is_empty() {
            self.average_price = None;
            self.min_price = None;
            self.max_price = None;
        } else {
            self.average_price = Some(prices.iter().sum::<f64>() / count_f64(prices.len()));
            self.min_price = prices.iter().copied().reduce(f64::min);
            self.max_price = prices.iter().copied().reduce(f64::max);
        }
        let occupied = dated
            .iter()
            .filter(|d| d.status() == DayStatus::Occupied)
            .count();
        let considered = dated
            .iter()
            .filter(|d| matches!(d.status(), DayStatus::Open | DayStatus::Occupied))
            .count();
        self.occupancy_rate = if considered == 0 {
            None
        } else {
            Some(count_f64(occupied) / count_f64(considered) * 100.0)
        };
    }

    /// Label past days against one explicit reference date.
    ///
    /// Every day dated before `today` becomes unavailable with
    /// `UnavailabilityReason::PastDate`: a past night cannot be booked, even
    /// if upstream flagged it available. Days on or after `today` that carry
    /// a stale `PastDate` label (for example from a cached calendar) are
    /// relabelled `Unknown`. Days with an unparseable date are left
    /// untouched. Statistics are recomputed.
    pub fn classify_past_days(&mut self, today: NaiveDate) {
        for day in &mut self.days {
            let Some(date) = day.parsed_date() else {
                continue;
            };
            if date < today {
                day.available = false;
                day.unavailability_reason = Some(UnavailabilityReason::PastDate);
            } else if day.unavailability_reason == Some(UnavailabilityReason::PastDate) {
                day.unavailability_reason = Some(UnavailabilityReason::Unknown);
            }
        }
        self.compute_stats();
    }

    /// Split the days into runs of consecutive dates.
    ///
    /// Parsers return days sorted by date with one entry per date; a missing
    /// date starts a new run. Gap and stay-length analytics must treat only
    /// days inside one run as adjacent nights.
    pub fn contiguous_runs(&self) -> Vec<&[CalendarDay]> {
        self.days
            .chunk_by(|previous, next| {
                matches!(
                    (
                        NaiveDate::parse_from_str(&previous.date, "%Y-%m-%d"),
                        NaiveDate::parse_from_str(&next.date, "%Y-%m-%d"),
                    ),
                    (Ok(previous), Ok(next)) if previous.succ_opt() == Some(next)
                )
            })
            .collect()
    }
}

/// A run of consecutive calendar days with the same availability, reason,
/// minimum stay and price: one row of the printed calendar.
#[derive(Debug, Clone)]
struct DayRun<'a> {
    first: &'a CalendarDay,
    last: &'a CalendarDay,
    nights: u32,
}

impl PriceCalendar {
    /// The days grouped into runs of consecutive dates with identical
    /// availability, reason, minimum stay and price. A missing or unparsable
    /// date starts a new run.
    fn day_runs(&self) -> Vec<DayRun<'_>> {
        let mut runs: Vec<DayRun<'_>> = Vec::new();
        for day in &self.days {
            let extends = runs.last().is_some_and(|run| {
                let consecutive = match (run.last.parsed_date(), day.parsed_date()) {
                    (Some(previous), Some(date)) => previous.succ_opt() == Some(date),
                    _ => false,
                };
                // The reason is printed for unavailable days only.
                let same_reason =
                    day.available || run.first.unavailability_reason == day.unavailability_reason;
                consecutive
                    && run.first.available == day.available
                    && same_reason
                    && run.first.min_nights == day.min_nights
                    && run.first.price == day.price
            });
            match runs.last_mut() {
                Some(run) if extends => {
                    run.last = day;
                    run.nights = run.nights.saturating_add(1);
                }
                _ => runs.push(DayRun {
                    first: day,
                    last: day,
                    nights: 1,
                }),
            }
        }
        runs
    }
}

impl std::fmt::Display for PriceCalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Price calendar for listing {} ({})",
            self.listing_id, self.currency
        )?;
        if let Some(occ) = self.occupancy_rate {
            let considered = self
                .days
                .iter()
                .filter(|d| {
                    d.parsed_date().is_some()
                        && matches!(d.status(), DayStatus::Open | DayStatus::Occupied)
                })
                .count();
            writeln!(
                f,
                "Occupancy: {occ:.1}% of {considered} future nights unavailable (past days excluded; bookings and host blocks are not distinguished)"
            )?;
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
        if !self.days.is_empty() && self.days.iter().all(|d| d.price.is_none()) {
            writeln!(
                f,
                "Nightly prices: unavailable (Airbnb's calendar does not publish per-day prices for this listing)"
            )?;
        }
        let runs = self.day_runs();
        writeln!(
            f,
            "{} days in {} date ranges (consecutive days with the same status are one row):",
            self.days.len(),
            runs.len()
        )?;
        writeln!(
            f,
            "{:<10}  {:<10}  {:>6}  {:<26}  {:>10}  {:>8}",
            "From", "To", "Nights", "Available", "Min nights", "Price"
        )?;
        writeln!(f, "{}", "-".repeat(80))?;
        for run in &runs {
            let day = run.first;
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
                "{:<10}  {:<10}  {:>6}  {:<26}  {:>10}  {:>8}",
                day.date, run.last.date, run.nights, available, min_nights, price
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
        // The row carries the date twice (from, to): 4 hyphens, plus the two
        // placeholders "-" for the minimum stay and the price.
        assert_eq!(day_line.matches('-').count(), 6);
        assert!(day_line.trim_end().ends_with('-'), "{day_line}");
    }

    fn stay_day(date: &str, available: bool, min_nights: Option<u32>) -> CalendarDay {
        CalendarDay {
            date: date.into(),
            price: None,
            available,
            min_nights,
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
            unavailability_reason: None,
        }
    }

    fn ranged_calendar(days: Vec<CalendarDay>) -> PriceCalendar {
        PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days,
            average_price: None,
            occupancy_rate: Some(40.0),
            min_price: None,
            max_price: None,
        }
    }

    #[test]
    fn calendar_display_groups_identical_consecutive_days_into_ranges() {
        let mut days = vec![
            stay_day("2027-06-01", false, Some(3)),
            stay_day("2027-06-02", false, Some(3)),
            stay_day("2027-06-03", true, Some(3)),
            stay_day("2027-06-04", true, Some(3)),
            stay_day("2027-06-05", true, Some(7)),
        ];
        let mut past = stay_day("2027-05-31", false, None);
        past.unavailability_reason = Some(UnavailabilityReason::PastDate);
        days.insert(0, past);
        let text = ranged_calendar(days).to_string();
        let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("20")).collect();
        assert_eq!(
            rows.len(),
            4,
            "past day, 2 unavailable, 2 open with a 3-night minimum, 1 with 7: {text}"
        );
        let cells = |row: &str| -> Vec<String> {
            row.split("  ")
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(String::from)
                .collect()
        };
        assert_eq!(
            cells(rows[0]),
            ["2027-05-31", "2027-05-31", "1", "No (Past date)", "-", "-"]
        );
        assert_eq!(
            cells(rows[1]),
            ["2027-06-01", "2027-06-02", "2", "No", "3", "-"]
        );
        assert_eq!(
            cells(rows[2]),
            ["2027-06-03", "2027-06-04", "2", "Yes", "3", "-"]
        );
        assert_eq!(
            cells(rows[3]),
            ["2027-06-05", "2027-06-05", "1", "Yes", "7", "-"]
        );
        assert!(
            text.contains("6 days in 4 date ranges"),
            "the header says how the rows were built: {text}"
        );
    }

    #[test]
    fn calendar_display_puts_the_summary_before_the_table() {
        let text = ranged_calendar(vec![stay_day("2027-06-01", true, None)]).to_string();
        let position = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {text}"))
        };
        assert!(position("Occupancy: 40.0%") < position("From "));
        assert!(position("Nightly prices: unavailable") < position("From "));
        assert!(position("From ") < position("2027-06-01"));
    }

    #[test]
    fn open_days_are_one_range_whatever_reason_they_still_carry() {
        let mut leftover = stay_day("2027-06-02", true, None);
        leftover.unavailability_reason = Some(UnavailabilityReason::Unknown);
        let cal = ranged_calendar(vec![
            stay_day("2027-06-01", true, None),
            leftover,
            stay_day("2027-06-03", true, None),
        ]);
        assert_eq!(cal.day_runs().len(), 1, "{cal}");
        // The reason still splits unavailable days: it is printed.
        let mut booked = stay_day("2027-06-02", false, None);
        booked.unavailability_reason = Some(UnavailabilityReason::Booked);
        let cal = ranged_calendar(vec![stay_day("2027-06-01", false, None), booked]);
        assert_eq!(cal.day_runs().len(), 2, "{cal}");
    }

    #[test]
    fn calendar_ranges_break_on_a_missing_date_a_price_change_or_a_bad_date() {
        let mut priced = stay_day("2027-06-04", true, None);
        priced.price = Some(120.0);
        let cal = ranged_calendar(vec![
            stay_day("2027-06-01", true, None),
            // 2027-06-02 is missing
            stay_day("2027-06-03", true, None),
            priced,
            stay_day("not-a-date", true, None),
            stay_day("2027-06-06", true, None),
        ]);
        let nights: Vec<u32> = cal.day_runs().iter().map(|run| run.nights).collect();
        assert_eq!(nights, [1, 1, 1, 1, 1]);
        assert!(cal.to_string().contains("$120"), "{cal}");

        let year: Vec<CalendarDay> = (0..365u64)
            .map(|offset| {
                let date = chrono::NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
                    + chrono::Days::new(offset);
                stay_day(&date.to_string(), offset % 100 < 60, Some(2))
            })
            .collect();
        let text = ranged_calendar(year).to_string();
        assert!(
            text.lines().count() < 20,
            "a year of long runs is a short table, not 365 lines: {} lines",
            text.lines().count()
        );
        assert_eq!(ranged_calendar(vec![]).day_runs().len(), 0);
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

    fn unpriced_day(date: &str) -> CalendarDay {
        CalendarDay {
            date: date.into(),
            price: None,
            available: true,
            min_nights: None,
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
            unavailability_reason: None,
        }
    }

    #[test]
    fn display_reports_unavailable_prices_when_no_day_is_priced() {
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![unpriced_day("2030-10-01"), unpriced_day("2030-10-02")],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        assert!(cal.to_string().contains("Nightly prices: unavailable"));
    }

    #[test]
    fn display_has_no_unavailable_notice_when_a_day_is_priced() {
        let mut priced = unpriced_day("2030-10-01");
        priced.price = Some(100.0);
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![priced, unpriced_day("2030-10-02")],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        assert!(!cal.to_string().contains("Nightly prices: unavailable"));
    }

    #[test]
    fn contiguous_runs_split_on_missing_dates() {
        let day = |date: &str| CalendarDay {
            date: date.into(),
            price: None,
            available: true,
            min_nights: None,
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
            unavailability_reason: None,
        };
        let cal = PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days: vec![
                day("2026-09-29"),
                day("2026-09-30"),
                day("2026-10-01"),
                day("2026-10-13"),
                day("2026-10-14"),
            ],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        let runs: Vec<Vec<&str>> = cal
            .contiguous_runs()
            .iter()
            .map(|run| run.iter().map(|d| d.date.as_str()).collect())
            .collect();
        assert_eq!(
            runs,
            vec![
                vec!["2026-09-29", "2026-09-30", "2026-10-01"],
                vec!["2026-10-13", "2026-10-14"],
            ]
        );
        let empty = PriceCalendar {
            days: vec![],
            ..cal.clone()
        };
        assert!(empty.contiguous_runs().is_empty());
    }

    #[test]
    fn contiguous_runs_isolate_a_day_with_an_unparseable_date() {
        let cal = calendar(vec![
            day("2026-10-01", true, None),
            day("2026-10-02", true, None),
            day("not a date", true, None),
            day("2026-10-03", true, None),
            day("2026-10-04", true, None),
        ]);
        let runs: Vec<Vec<&str>> = cal
            .contiguous_runs()
            .iter()
            .map(|run| run.iter().map(|d| d.date.as_str()).collect())
            .collect();
        assert_eq!(
            runs,
            vec![
                vec!["2026-10-01", "2026-10-02"],
                vec!["not a date"],
                vec!["2026-10-03", "2026-10-04"],
            ]
        );
    }

    fn day(date: &str, available: bool, reason: Option<UnavailabilityReason>) -> CalendarDay {
        CalendarDay {
            date: date.into(),
            price: None,
            available,
            min_nights: None,
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
            unavailability_reason: reason,
        }
    }

    fn calendar(days: Vec<CalendarDay>) -> PriceCalendar {
        PriceCalendar {
            listing_id: "1".into(),
            currency: "$".into(),
            days,
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        }
    }

    #[test]
    fn status_classifies_each_reason() {
        assert_eq!(day("2026-10-01", true, None).status(), DayStatus::Open);
        assert_eq!(
            day("2026-09-01", false, Some(UnavailabilityReason::PastDate)).status(),
            DayStatus::Past
        );
        assert_eq!(
            day(
                "2026-10-01",
                false,
                Some(UnavailabilityReason::BlockedByHost)
            )
            .status(),
            DayStatus::Blocked
        );
        assert_eq!(
            day(
                "2026-10-01",
                false,
                Some(UnavailabilityReason::MinNightRestriction)
            )
            .status(),
            DayStatus::Blocked
        );
        assert_eq!(
            day("2026-10-01", false, Some(UnavailabilityReason::Booked)).status(),
            DayStatus::Occupied
        );
        assert_eq!(
            day("2026-10-01", false, Some(UnavailabilityReason::Unknown)).status(),
            DayStatus::Occupied
        );
        assert_eq!(day("2026-10-01", false, None).status(), DayStatus::Occupied);
    }

    #[test]
    fn compute_stats_excludes_past_days_from_occupancy() {
        let mut days: Vec<CalendarDay> = (1..=27)
            .map(|d| {
                day(
                    &format!("2026-09-{d:02}"),
                    false,
                    Some(UnavailabilityReason::PastDate),
                )
            })
            .collect();
        days.extend((28..=30).map(|d| day(&format!("2026-09-{d:02}"), true, None)));
        let mut cal = calendar(days);
        cal.compute_stats();
        assert!(cal.occupancy_rate.unwrap().abs() < 1e-9);
    }

    #[test]
    fn compute_stats_without_future_nights_has_no_occupancy() {
        let days: Vec<CalendarDay> = (1..=27)
            .map(|d| {
                day(
                    &format!("2026-09-{d:02}"),
                    false,
                    Some(UnavailabilityReason::PastDate),
                )
            })
            .collect();
        let mut cal = calendar(days);
        cal.compute_stats();
        assert!(cal.occupancy_rate.is_none());
    }

    #[test]
    fn classify_past_days_uses_the_reference_date() {
        let mut cal = calendar(vec![
            // A past night flagged available upstream cannot be booked any more.
            day("2026-09-26", true, None),
            day("2026-09-27", false, Some(UnavailabilityReason::Unknown)),
            // Stale label from an earlier run anchored on another date.
            day("2026-09-28", false, Some(UnavailabilityReason::PastDate)),
            day("2026-09-29", true, None),
            day("not-a-date", false, Some(UnavailabilityReason::Unknown)),
        ]);
        cal.classify_past_days(NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());

        assert!(!cal.days[0].available);
        assert_eq!(
            cal.days[0].unavailability_reason,
            Some(UnavailabilityReason::PastDate)
        );
        assert_eq!(
            cal.days[1].unavailability_reason,
            Some(UnavailabilityReason::PastDate)
        );
        assert_eq!(
            cal.days[2].unavailability_reason,
            Some(UnavailabilityReason::Unknown)
        );
        assert_eq!(cal.days[3].unavailability_reason, None);
        assert_eq!(
            cal.days[4].unavailability_reason,
            Some(UnavailabilityReason::Unknown)
        );
        // Stats were recomputed: 09-28 occupied + 09-29 open.
        assert!((cal.occupancy_rate.unwrap() - 50.0).abs() < 1e-9);
    }
}
