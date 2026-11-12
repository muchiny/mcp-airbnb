//! Dated price quote: what a guest pays for one listing on given dates.
//!
//! Airbnb's calendar no longer publishes nightly prices. The booking panel
//! of the listing page still does, for one stay at a time: a quote is that
//! panel's answer for a check-in, a check-out and a number of adults.

use chrono::{Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::limits;
use crate::domain::listing_id::validate_listing_id;
use crate::domain::search_params::check_stay_dates;
use crate::error::{AirbnbError, Result};

/// The stay a quote is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteRequest {
    /// Airbnb listing id.
    pub listing_id: String,
    /// Check-in date, `YYYY-MM-DD`.
    pub checkin: String,
    /// Check-out date, `YYYY-MM-DD`.
    pub checkout: String,
    /// Number of adults, 1 to `limits::MAX_ADULTS`.
    pub adults: u32,
}

impl QuoteRequest {
    /// Validate against today's UTC date. See [`QuoteRequest::validate_at`].
    pub fn validate(&self) -> Result<u32> {
        self.validate_at(Utc::now().date_naive())
    }

    /// Check the listing id, the stay (same rules as a dated search) and the
    /// number of adults. Returns the number of nights.
    pub fn validate_at(&self, today: NaiveDate) -> Result<u32> {
        validate_listing_id(&self.listing_id)?;
        let (checkin, checkout) = check_stay_dates(&self.checkin, &self.checkout, today)?;
        limits::in_range("adults", self.adults, 1, limits::MAX_ADULTS)?;
        let nights = checkout.signed_duration_since(checkin).num_days();
        u32::try_from(nights).map_err(|_| AirbnbError::InvalidParams {
            reason: "checkout date must be after checkin date".into(),
        })
    }

    /// The same stay `weeks` weeks later, or `None` when a date does not
    /// parse or the shift overflows the calendar.
    pub fn shifted_by_weeks(&self, weeks: u32) -> Option<Self> {
        let days = Days::new(u64::from(weeks).checked_mul(7)?);
        let shift = |date: &str| -> Option<String> {
            let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
            Some(
                parsed
                    .checked_add_days(days)?
                    .format("%Y-%m-%d")
                    .to_string(),
            )
        };
        Some(Self {
            listing_id: self.listing_id.clone(),
            checkin: shift(&self.checkin)?,
            checkout: shift(&self.checkout)?,
            adults: self.adults,
        })
    }
}

/// One line of the price breakdown, as Airbnb prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceLine {
    /// What the line covers (`7 nights x $297.34`, `Cleaning fee`, `Total`).
    pub description: String,
    /// Amount of the line, with Airbnb's own currency formatting.
    pub amount: String,
}

/// Airbnb's answer for one stay of one listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceQuote {
    /// Airbnb listing id.
    pub listing_id: String,
    /// Check-in date, `YYYY-MM-DD`.
    pub checkin: String,
    /// Check-out date, `YYYY-MM-DD`.
    pub checkout: String,
    /// Number of nights of the stay.
    pub nights: u32,
    /// Number of adults the quote is for.
    pub adults: u32,
    /// Whether the stay can be booked.
    pub available: bool,
    /// Why the stay cannot be booked (`Minimum stay is 7 nights`).
    #[serde(default)]
    pub unavailability_message: Option<String>,
    /// Whether the stay can be booked without the host's approval.
    #[serde(default)]
    pub instant_book: Option<bool>,
    /// Total of the stay as Airbnb prints it (`$2,082`). `None` when the
    /// panel shows no price, which is the usual case of an unavailable stay.
    #[serde(default)]
    pub total_display: Option<String>,
    /// The same total as a number.
    #[serde(default)]
    pub total: Option<f64>,
    /// Total divided by the number of nights: fees and taxes included.
    #[serde(default)]
    pub nightly: Option<f64>,
    /// Nightly price on the basis search results use (the panel's
    /// `N nights x price` line when it has one, before fees). Compare this
    /// one with the prices of other listings, never `nightly`.
    #[serde(default)]
    pub listed_nightly: Option<f64>,
    /// Currency label of the amounts.
    pub currency: String,
    /// Price breakdown lines.
    #[serde(default)]
    pub breakdown: Vec<PriceLine>,
    /// Messages of the booking panel (cancellation, "rare find", …).
    #[serde(default)]
    pub notes: Vec<String>,
}

impl PriceQuote {
    /// `yes`, `yes (Instant Book)`, `no` or `no (reason)`.
    fn availability_label(&self) -> String {
        match (self.available, &self.unavailability_message) {
            (true, _) if self.instant_book == Some(true) => "yes (Instant Book)".to_string(),
            (true, _) => "yes".to_string(),
            (false, Some(message)) => format!("no ({message})"),
            (false, None) => "no".to_string(),
        }
    }
}

/// `"1 night"` / `"7 nights"`.
fn plural(count: u32, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

impl std::fmt::Display for PriceQuote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Price quote: listing {}", self.listing_id)?;
        writeln!(
            f,
            "Stay: {} to {} ({}, {})",
            self.checkin,
            self.checkout,
            plural(self.nights, "night"),
            plural(self.adults, "adult")
        )?;
        writeln!(f, "Available: {}", self.availability_label())?;
        match (&self.total_display, self.nightly) {
            (Some(total), Some(nightly)) => {
                writeln!(
                    f,
                    "Total: {total} ({}{nightly:.2} per night on average, fees and taxes included)",
                    self.currency
                )?;
                // A rounding difference is not a fee: say it from 0.5 % up.
                if let Some(listed) = self
                    .listed_nightly
                    .filter(|listed| (listed - nightly).abs() > nightly * 0.005)
                {
                    writeln!(
                        f,
                        "Nightly rate before fees and taxes: {}{listed:.2} (the price search results show)",
                        self.currency
                    )?;
                }
            }
            (Some(total), None) => writeln!(f, "Total: {total}")?,
            (None, _) => writeln!(f, "Total: no price (Airbnb quotes bookable stays only)")?,
        }
        if !self.breakdown.is_empty() {
            writeln!(f, "Price details:")?;
            for line in &self.breakdown {
                writeln!(f, "  - {}: {}", line.description, line.amount)?;
            }
        }
        if !self.notes.is_empty() {
            writeln!(f, "Notes:")?;
            for note in &self.notes {
                writeln!(f, "  - {note}")?;
            }
        }
        Ok(())
    }
}

/// Quotes of the same stay over consecutive weeks: the seasonal price curve
/// the calendar no longer gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteSeries {
    /// Airbnb listing id.
    pub listing_id: String,
    /// One quote per week, in date order.
    pub quotes: Vec<PriceQuote>,
}

impl QuoteSeries {
    /// Bookable quotes with a nightly price, as `(check-in, nightly)`.
    fn priced(&self) -> impl Iterator<Item = (&str, f64)> {
        self.quotes
            .iter()
            .filter_map(|quote| Some((quote.checkin.as_str(), quote.nightly?)))
    }
}

impl std::fmt::Display for QuoteSeries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((first, rest)) = self.quotes.split_first() else {
            return writeln!(f, "# Price quotes: listing {} (none)", self.listing_id);
        };
        if rest.is_empty() {
            return write!(f, "{first}");
        }
        writeln!(
            f,
            "# Price quotes: listing {} ({} weekly stays of {}, {})",
            self.listing_id,
            self.quotes.len(),
            plural(first.nights, "night"),
            plural(first.adults, "adult")
        )?;
        writeln!(
            f,
            "| Check-in | Check-out | Available | Total | Per night |"
        )?;
        writeln!(f, "|---|---|---|---|---|")?;
        for quote in &self.quotes {
            let total = quote.total_display.as_deref().unwrap_or("-");
            let nightly = quote.nightly.map_or_else(
                || "-".to_string(),
                |nightly| format!("{}{nightly:.2}", quote.currency),
            );
            writeln!(
                f,
                "| {} | {} | {} | {total} | {nightly} |",
                quote.checkin,
                quote.checkout,
                quote.availability_label()
            )?;
        }
        let cheapest = self.priced().min_by(|a, b| a.1.total_cmp(&b.1));
        let dearest = self.priced().max_by(|a, b| a.1.total_cmp(&b.1));
        if let (Some((low_date, low)), Some((high_date, high))) = (cheapest, dearest) {
            writeln!(
                f,
                "\nLowest: {currency}{low:.2} per night (stay from {low_date}) | Highest: {currency}{high:.2} per night (stay from {high_date})",
                currency = first.currency
            )?;
        } else {
            writeln!(f, "\nNo stay of the series can be booked.")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()
    }

    fn request() -> QuoteRequest {
        QuoteRequest {
            listing_id: "45490342".into(),
            checkin: "2027-06-12".into(),
            checkout: "2027-06-19".into(),
            adults: 2,
        }
    }

    fn quote(checkin: &str, checkout: &str, nightly: Option<f64>) -> PriceQuote {
        PriceQuote {
            listing_id: "45490342".into(),
            checkin: checkin.into(),
            checkout: checkout.into(),
            nights: 7,
            adults: 2,
            available: nightly.is_some(),
            unavailability_message: nightly
                .is_none()
                .then(|| "Those dates are not available".to_string()),
            instant_book: Some(nightly.is_some()),
            total_display: nightly.map(|n| format!("${:.0}", n * 7.0)),
            total: nightly.map(|n| n * 7.0),
            nightly,
            listed_nightly: None,
            currency: "$".into(),
            breakdown: vec![],
            notes: vec![],
        }
    }

    #[test]
    fn a_valid_request_yields_its_number_of_nights() {
        assert_eq!(request().validate_at(today()).unwrap(), 7);
    }

    #[test]
    fn invalid_requests_are_rejected_not_adjusted() {
        type Change = fn(&mut QuoteRequest);
        let cases: [(&str, Change); 5] = [
            ("must contain only ASCII digits", |r| {
                r.listing_id = "abc".into();
            }),
            ("adults must be between 1 and 16", |r| r.adults = 0),
            ("adults must be between 1 and 16", |r| r.adults = 17),
            ("checkout date must be after checkin date", |r| {
                r.checkout = "2027-06-12".into();
            }),
            ("in the past", |r| {
                r.checkin = "2026-01-01".into();
                r.checkout = "2026-01-05".into();
            }),
        ];
        for (expected, change) in cases {
            let mut bad = request();
            change(&mut bad);
            let message = bad.validate_at(today()).unwrap_err().to_string();
            assert!(message.contains(expected), "{expected}: {message}");
        }
    }

    #[test]
    fn shifting_by_weeks_moves_both_dates() {
        let later = request().shifted_by_weeks(3).unwrap();
        assert_eq!(later.checkin, "2027-07-03");
        assert_eq!(later.checkout, "2027-07-10");
        assert_eq!(request().shifted_by_weeks(0).unwrap(), request());
        let mut broken = request();
        broken.checkin = "soon".into();
        assert_eq!(broken.shifted_by_weeks(1), None);
    }

    #[test]
    fn a_bookable_quote_prints_total_average_and_details() {
        let mut bookable = quote("2027-06-12", "2027-06-19", Some(297.43));
        bookable.total_display = Some("$2,082".into());
        bookable.breakdown = vec![PriceLine {
            description: "7 nights x $297.34".into(),
            amount: "$2,081.37".into(),
        }];
        bookable.notes = vec!["Rare find! This place is usually booked".into()];
        let text = bookable.to_string();
        assert!(
            text.contains("Stay: 2027-06-12 to 2027-06-19 (7 nights, 2 adults)"),
            "{text}"
        );
        assert!(text.contains("Available: yes (Instant Book)"), "{text}");
        assert!(
            text.contains("Total: $2,082 ($297.43 per night on average, fees and taxes included)"),
            "{text}"
        );
        assert!(text.contains("  - 7 nights x $297.34: $2,081.37"), "{text}");
        assert!(text.contains("  - Rare find!"), "{text}");
    }

    #[test]
    fn an_unbookable_quote_says_why_and_shows_no_price() {
        let text = quote("2027-07-10", "2027-07-17", None).to_string();
        assert!(
            text.contains("Available: no (Those dates are not available)"),
            "{text}"
        );
        assert!(text.contains("Total: no price"), "{text}");
    }

    #[test]
    fn a_series_prints_a_table_with_its_extremes() {
        let series = QuoteSeries {
            listing_id: "45490342".into(),
            quotes: vec![
                quote("2027-06-12", "2027-06-19", Some(200.0)),
                quote("2027-06-19", "2027-06-26", None),
                quote("2027-06-26", "2027-07-03", Some(320.5)),
            ],
        };
        let text = series.to_string();
        assert!(
            text.contains("3 weekly stays of 7 nights, 2 adults"),
            "{text}"
        );
        assert!(
            text.contains("| 2027-06-12 | 2027-06-19 | yes (Instant Book) | $1400 | $200.00 |"),
            "{text}"
        );
        assert!(
            text.contains(
                "| 2027-06-19 | 2027-06-26 | no (Those dates are not available) | - | - |"
            ),
            "{text}"
        );
        assert!(text.contains("Lowest: $200.00 per night (stay from 2027-06-12) | Highest: $320.50 per night (stay from 2027-06-26)"), "{text}");
    }

    #[test]
    fn a_single_quote_series_prints_the_quote_itself() {
        let only = quote("2027-06-12", "2027-06-19", Some(200.0));
        let series = QuoteSeries {
            listing_id: "45490342".into(),
            quotes: vec![only.clone()],
        };
        assert_eq!(series.to_string(), only.to_string());
        let unbookable = QuoteSeries {
            listing_id: "45490342".into(),
            quotes: vec![
                quote("2027-06-12", "2027-06-19", None),
                quote("2027-06-19", "2027-06-26", None),
            ],
        };
        assert!(
            unbookable
                .to_string()
                .contains("No stay of the series can be booked.")
        );
    }
}
