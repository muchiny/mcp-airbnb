//! Position of a listing in Airbnb's search results.
//!
//! Airbnb's order is not stable: it changes with the dates, the number of
//! guests, the session and the locale. A rank is one measure, to be repeated
//! before two ranks are compared.

use serde::{Deserialize, Serialize};

use crate::domain::listing::Listing;
use crate::domain::num::count_u32;

/// Results kept on each side of the ranked listing.
const NEIGHBORS_EACH_SIDE: usize = 3;
/// Top results shown when the listing was not found.
const TOP_WHEN_ABSENT: usize = 5;

/// A search result near the ranked listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedNeighbor {
    /// 1-based position in the results.
    pub position: u32,
    /// Airbnb listing id.
    pub id: String,
    /// Listing name.
    pub name: String,
    /// Nightly price of the search card, when known.
    pub price_per_night: Option<f64>,
    /// Rating of the search card.
    pub rating: Option<f64>,
    /// Number of reviews of the search card.
    pub review_count: u32,
    /// Badges of the search card.
    #[serde(default)]
    pub badges: Vec<String>,
}

/// What a rank measure scanned.
#[derive(Debug, Clone)]
pub struct RankScan<'a> {
    /// The listing to locate.
    pub listing_id: &'a str,
    /// The search, in words (location, dates, guests, filters).
    pub search: String,
    /// The results in Airbnb's order, unique by id.
    pub listings: &'a [Listing],
    /// Search pages fetched.
    pub pages_fetched: u32,
    /// Page on which the listing was found (1-based), as counted by the
    /// scan. A page can hold fewer or more than 18 distinct results, so the
    /// page is never derived from the position.
    pub found_on_page: Option<u32>,
    /// Whether Airbnb had no further page.
    pub exhausted: bool,
}

/// Where a listing sits in the results of one search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListingRank {
    /// The ranked listing.
    pub listing_id: String,
    /// The search, in words.
    pub search: String,
    /// 1-based position, or `None` when the listing is not in the scanned results.
    pub position: Option<u32>,
    /// Results page on which the listing was found (1-based).
    pub page: Option<u32>,
    /// Results scanned.
    pub scanned: u32,
    /// Search pages fetched.
    pub pages_fetched: u32,
    /// Whether Airbnb had no further page (the scan saw every result).
    pub exhausted: bool,
    /// Currency label of the prices.
    pub currency: String,
    /// The listing's own nightly price in this search.
    pub listing_price: Option<f64>,
    /// Median nightly price of the scanned results that have one.
    pub median_price: Option<f64>,
    /// Scanned results cheaper than the listing (when its price is known).
    pub cheaper_count: Option<u32>,
    /// Scanned results with a price.
    pub priced_count: u32,
    /// Results around the listing, or the top results when it is absent.
    #[serde(default)]
    pub neighbors: Vec<RankedNeighbor>,
}

fn neighbor(index: usize, listing: &Listing) -> RankedNeighbor {
    RankedNeighbor {
        position: count_u32(index.saturating_add(1)),
        id: listing.id.clone(),
        name: listing.name.clone(),
        price_per_night: listing.known_price(),
        rating: listing.rating,
        review_count: listing.review_count,
        badges: listing.badges().into_iter().map(String::from).collect(),
    }
}

/// Median of `values`, or `None` when empty.
fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    let upper = *values.get(mid)?;
    if values.len().is_multiple_of(2) {
        let lower = *values.get(mid.checked_sub(1)?)?;
        Some(f64::midpoint(lower, upper))
    } else {
        Some(upper)
    }
}

/// Locate a listing in scanned search results and compare its price with
/// the results around it.
pub fn compute_listing_rank(scan: &RankScan<'_>) -> ListingRank {
    let index = scan
        .listings
        .iter()
        .position(|listing| listing.id == scan.listing_id);
    let own = index.and_then(|i| scan.listings.get(i));
    let listing_price = own.and_then(Listing::known_price);
    let prices: Vec<f64> = scan
        .listings
        .iter()
        .filter_map(Listing::known_price)
        .collect();
    let cheaper_count = listing_price
        .map(|own_price| count_u32(prices.iter().filter(|price| **price < own_price).count()));
    let currency = own
        .or_else(|| scan.listings.first())
        .map_or_default(|listing| listing.currency.clone());

    let window = match index {
        Some(i) => {
            i.saturating_sub(NEIGHBORS_EACH_SIDE)
                ..i.saturating_add(NEIGHBORS_EACH_SIDE).saturating_add(1)
        }
        None => 0..TOP_WHEN_ABSENT,
    };
    let neighbors = scan
        .listings
        .iter()
        .enumerate()
        .skip(window.start)
        .take(window.end.saturating_sub(window.start))
        .map(|(i, listing)| neighbor(i, listing))
        .collect();

    ListingRank {
        listing_id: scan.listing_id.to_string(),
        search: scan.search.clone(),
        position: index.map(|i| count_u32(i.saturating_add(1))),
        page: index.and(scan.found_on_page),
        scanned: count_u32(scan.listings.len()),
        pages_fetched: scan.pages_fetched,
        exhausted: scan.exhausted,
        currency,
        listing_price,
        median_price: median(prices.clone()),
        cheaper_count,
        priced_count: count_u32(prices.len()),
        neighbors,
    }
}

impl std::fmt::Display for ListingRank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# Search rank: listing {}", self.listing_id)?;
        writeln!(f, "Search: {}", self.search)?;
        match (self.position, self.page) {
            (Some(position), Some(page)) => writeln!(
                f,
                "Position: {position} of {} results scanned (page {page})",
                self.scanned
            )?,
            _ if self.exhausted => writeln!(
                f,
                "Position: not in the results (Airbnb returned {} results over {} page(s), all scanned)",
                self.scanned, self.pages_fetched
            )?,
            _ => writeln!(
                f,
                "Position: not in the first {} results ({} page(s) scanned; raise max_pages to look further, Airbnb serves at most 15 pages)",
                self.scanned, self.pages_fetched
            )?,
        }
        match (self.listing_price, self.median_price) {
            (Some(own), Some(median)) => {
                write!(
                    f,
                    "Price: {c}{own:.0}/night | median of the scanned results: {c}{median:.0}/night",
                    c = self.currency
                )?;
                if let Some(cheaper) = self.cheaper_count {
                    write!(
                        f,
                        " | {cheaper} of {} priced results are cheaper",
                        self.priced_count
                    )?;
                }
                writeln!(f)?;
            }
            (None, Some(median)) => writeln!(
                f,
                "Median price of the scanned results: {}{median:.0}/night ({} priced)",
                self.currency, self.priced_count
            )?,
            _ => writeln!(
                f,
                "Prices: none published (run the measure with checkin and checkout)"
            )?,
        }
        if !self.neighbors.is_empty() {
            let title = if self.position.is_some() {
                "Around it:"
            } else {
                "Top results:"
            };
            writeln!(f, "{title}")?;
            for n in &self.neighbors {
                let marker = if Some(n.position) == self.position {
                    ">"
                } else {
                    " "
                };
                write!(f, "{marker} {}. {} (ID: {})", n.position, n.name, n.id)?;
                if let Some(price) = n.price_per_night {
                    write!(f, " | {}{price:.0}/night", self.currency)?;
                }
                if let Some(rating) = n.rating {
                    write!(f, " | {rating:.2} ({} reviews)", n.review_count)?;
                }
                for badge in &n.badges {
                    write!(f, " | {badge}")?;
                }
                writeln!(f)?;
            }
        }
        writeln!(
            f,
            "Note: Airbnb's order changes with the dates, the guests, the session and the locale. Repeat the measure before comparing two ranks."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_listing;

    fn results(count: usize) -> Vec<Listing> {
        (1..=count)
            .map(|n| {
                let price = 100.0 + 10.0 * f64::from(u32::try_from(n).unwrap());
                make_listing(&n.to_string(), &format!("Flat {n}"), price)
            })
            .collect()
    }

    fn scan<'a>(id: &'a str, listings: &'a [Listing], exhausted: bool) -> RankScan<'a> {
        RankScan {
            listing_id: id,
            search: "Paris | 2027-06-12 to 2027-06-19 | 2 adults".into(),
            listings,
            pages_fetched: 2,
            // Full pages of 18 distinct results, unless a test says otherwise.
            found_on_page: listings
                .iter()
                .position(|listing| listing.id == id)
                .map(|index| count_u32(index / 18 + 1)),
            exhausted,
        }
    }

    #[test]
    fn the_page_is_the_one_the_scan_found_the_listing_on() {
        // 17 distinct results on page 1 (one card was a repeat or unreadable),
        // the listing first on page 2: position 18, page 2, not page 1.
        let listings = results(18);
        let rank = compute_listing_rank(&RankScan {
            found_on_page: Some(2),
            ..scan("18", &listings, false)
        });
        assert_eq!(rank.position, Some(18));
        assert_eq!(rank.page, Some(2));
        // A page number without a position is never reported.
        let absent = compute_listing_rank(&RankScan {
            found_on_page: Some(2),
            ..scan("999", &listings, false)
        });
        assert_eq!((absent.position, absent.page), (None, None));
    }

    #[test]
    fn a_found_listing_has_a_position_a_page_and_a_price_comparison() {
        let listings = results(30);
        let rank = compute_listing_rank(&scan("20", &listings, false));
        assert_eq!(rank.position, Some(20));
        assert_eq!(rank.page, Some(2), "the page the scan found it on");
        assert_eq!(rank.scanned, 30);
        assert_eq!(rank.listing_price, Some(300.0));
        assert_eq!(rank.cheaper_count, Some(19));
        assert_eq!(rank.priced_count, 30);
        assert_eq!(rank.median_price, Some(255.0));
        let positions: Vec<u32> = rank.neighbors.iter().map(|n| n.position).collect();
        assert_eq!(positions, [17, 18, 19, 20, 21, 22, 23]);
        let text = rank.to_string();
        assert!(
            text.contains("Position: 20 of 30 results scanned (page 2)"),
            "{text}"
        );
        assert!(
            text.contains("> 20. Flat 20 (ID: 20) | $300/night"),
            "{text}"
        );
        assert!(
            text.contains("19 of 30 priced results are cheaper"),
            "{text}"
        );
    }

    #[test]
    fn the_first_and_the_last_result_keep_a_clipped_window() {
        let listings = results(18);
        let first = compute_listing_rank(&scan("1", &listings, false));
        assert_eq!(first.position, Some(1));
        assert_eq!(first.page, Some(1));
        assert_eq!(first.neighbors.len(), 4);
        let last = compute_listing_rank(&scan("18", &listings, false));
        assert_eq!(last.page, Some(1), "position 18 closes page 1");
        assert_eq!(last.neighbors.len(), 4);
    }

    #[test]
    fn an_absent_listing_says_whether_everything_was_scanned() {
        let listings = results(8);
        let partial = compute_listing_rank(&scan("999", &listings, false));
        assert_eq!(partial.position, None);
        assert_eq!(partial.cheaper_count, None);
        assert_eq!(partial.neighbors.len(), 5);
        let text = partial.to_string();
        assert!(text.contains("not in the first 8 results"), "{text}");
        assert!(text.contains("Top results:"), "{text}");
        assert!(
            text.contains("Median price of the scanned results: $145/night"),
            "{text}"
        );

        let complete = compute_listing_rank(&scan("999", &listings, true));
        assert!(complete.to_string().contains("all scanned"), "{complete}");
    }

    #[test]
    fn an_empty_or_unpriced_scan_has_no_price_line() {
        let empty = compute_listing_rank(&scan("1", &[], true));
        assert_eq!(empty.scanned, 0);
        assert_eq!(empty.median_price, None);
        assert_eq!(empty.currency, "");
        assert!(empty.to_string().contains("Prices: none published"));

        let mut unpriced = results(3);
        for listing in &mut unpriced {
            listing.price_per_night = 0.0;
        }
        let rank = compute_listing_rank(&scan("2", &unpriced, true));
        assert_eq!(rank.position, Some(2));
        assert_eq!(rank.listing_price, None);
        assert_eq!(rank.priced_count, 0);
    }
}
