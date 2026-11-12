//! `StaysPdpBookItQuery`: the booking panel of a listing for one stay.
//!
//! The panel answers with a price only when the stay can be booked. When it
//! cannot, it says why (`Minimum stay is 7 nights`, `Those dates are not
//! available`, `Maximum number of guests is 4`): that is a valid quote, not
//! an error.

use base64::Engine as _;
use serde_json::Value;

use crate::adapters::price;
use crate::adapters::text;
use crate::domain::quote::{PriceLine, PriceQuote, QuoteRequest};
use crate::error::{AirbnbError, Result};

/// Name of the persisted query.
pub const BOOK_IT_OPERATION: &str = "StaysPdpBookItQuery";

/// The booking panel inside the response.
const BOOK_IT_POINTER: &str = "/data/node/pdpPresentation/bookIt";

/// Variables of `StaysPdpBookItQuery` as the airbnb.com web client sends
/// them for the booking sidebar (captured 2026-11-12), without the optional
/// fragments the quote does not read.
pub fn build_quote_variables(request: &QuoteRequest) -> Value {
    let id = base64::engine::general_purpose::STANDARD
        .encode(format!("DemandStayListing:{}", request.listing_id));
    serde_json::json!({
        "id": id,
        "dateRange": {"startDate": request.checkin, "endDate": request.checkout},
        "guestCounts": {"numberOfAdults": request.adults},
        "includePdpMigrationBookItCalendarSheetFragment": false,
        "includePdpMigrationBookItFloatingFooterFragment": false,
        "includePdpMigrationBookItNavFragment": false,
        "includePdpMigrationBookItSidebarFragment": true,
        "includeGalleryBookItCtaFragment": false,
        "includeOverviewMerchandisingTipsFragment": false,
        "includeStaysPdpPriceHeatmapFragment": false,
        "priceHeatmapDateRange": null,
        "p3ImpressionId": null,
        "selectedCancellationPolicyId": null,
        "causeId": null,
        "selectedGuestOptionId": null,
        "federatedSearchId": null,
        "interactionType": null,
        "logEntityPricingViewEvent": false,
    })
}

fn drift(detail: &str) -> AirbnbError {
    AirbnbError::UpstreamSchema {
        operation: BOOK_IT_OPERATION.into(),
        detail: detail.into(),
    }
}

/// Non-empty string at `pointer`, with HTML tags removed.
fn plain(node: &Value, pointer: &str) -> Option<String> {
    node.pointer(pointer)
        .and_then(Value::as_str)
        .map(text::html_to_text)
        .filter(|value| !value.trim().is_empty())
}

/// Most panel messages read from one response.
const MAX_PANEL_NOTES: usize = 50;

/// Breakdown lines of one `explanationData.priceDetails` array.
fn breakdown_lines(details: Option<&Value>) -> Vec<PriceLine> {
    details
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("items").and_then(Value::as_array))
        .flatten()
        .filter_map(|item| {
            Some(PriceLine {
                description: plain(item, "/description")?,
                amount: plain(item, "/priceString")?,
            })
        })
        .collect()
}

/// Messages of the panel: price disclaimers, the payment tip and the
/// announcement, in that order and without duplicates.
fn panel_notes(book_it: &Value) -> Vec<String> {
    let disclaimers = book_it
        .pointer("/structuredDisplayPrice/explanationData/priceDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| plain(entry, "/content"));
    let tips = ["/integratedTip/title", "/announcement/title"]
        .into_iter()
        .filter_map(|pointer| plain(book_it, pointer));
    let mut notes: Vec<String> = Vec::new();
    // Capped: the scan below is linear, and a real panel has a few notes.
    for note in disclaimers.chain(tips).take(MAX_PANEL_NOTES) {
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    notes
}

/// Parse the booking panel of one stay.
///
/// `nights` is the length of the stay and `default_currency` the symbol of
/// the pinned currency, used when Airbnb prints an amount without a label.
///
/// # Errors
///
/// [`AirbnbError::UpstreamSchema`] when the response has no booking panel,
/// when the panel does not say whether the stay is available, or when a
/// bookable stay comes without a price.
pub fn parse_quote_response(
    json: &Value,
    request: &QuoteRequest,
    nights: u32,
    default_currency: &str,
) -> Result<PriceQuote> {
    let book_it = json
        .pointer(BOOK_IT_POINTER)
        .filter(|node| node.is_object())
        .ok_or_else(|| drift("response has no data.node.pdpPresentation.bookIt object"))?;
    let available = book_it
        .pointer("/availability/isAvailable")
        .and_then(Value::as_bool)
        .ok_or_else(|| drift("bookIt has no availability.isAvailable"))?;

    let display_block = book_it
        .get("structuredDisplayPrice")
        .filter(|node| node.is_object());
    let display = display_block
        .map(|block| price::parse_structured_display_price(block, Some(nights)))
        .unwrap_or_default();
    let total = display
        .total
        .or_else(|| display.nightly.map(|nightly| nightly * f64::from(nights)));
    if available && total.is_none() {
        return Err(drift("a bookable stay came without a price"));
    }
    let currency = display
        .currency
        .unwrap_or_else(|| default_currency.to_string());
    // Airbnb's own total string when the primary line is the total; a
    // formatted amount when the panel printed a nightly price instead.
    let printed_total = display_block
        .and_then(|block| block.pointer("/primaryLine"))
        .and_then(price::primary_price_str)
        .filter(|printed| price::parse_price_amount(printed) == total);
    let total_display = match (printed_total, total) {
        (Some(printed), _) => Some(printed.to_string()),
        (None, Some(amount)) => Some(format!("{currency}{amount:.2}")),
        (None, None) => None,
    };
    let nightly = total
        .filter(|_| nights > 0)
        .map(|amount| amount / f64::from(nights));

    let itemized =
        breakdown_lines(book_it.pointer("/productItemDetail/explanationData/priceDetails"));
    let breakdown = if itemized.is_empty() {
        breakdown_lines(book_it.pointer("/structuredDisplayPrice/explanationData/priceDetails"))
    } else {
        itemized
    };

    Ok(PriceQuote {
        listing_id: request.listing_id.clone(),
        checkin: request.checkin.clone(),
        checkout: request.checkout.clone(),
        nights,
        adults: request.adults,
        available,
        unavailability_message: plain(book_it, "/availability/unavailabilityMessage"),
        instant_book: book_it
            .pointer("/availability/canInstantBook")
            .and_then(Value::as_bool),
        total_display,
        total,
        nightly,
        listed_nightly: display.nightly,
        currency,
        breakdown,
        notes: panel_notes(book_it),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::fixture_2026_11;

    fn request() -> QuoteRequest {
        QuoteRequest {
            listing_id: "45490342".into(),
            checkin: "2027-06-12".into(),
            checkout: "2027-06-19".into(),
            adults: 2,
        }
    }

    fn unavailable(message: &str) -> Value {
        serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
            "structuredDisplayPrice": null,
            "availability": {
                "isAvailable": false,
                "canInstantBook": false,
                "unavailabilityMessage": message,
                "unavailabilityReason": "DATE"
            },
            "integratedTip": null,
            "announcement": null
        }}}}})
    }

    #[test]
    fn variables_name_the_listing_the_stay_and_the_guests() {
        let variables = build_quote_variables(&request());
        assert_eq!(variables["id"], "RGVtYW5kU3RheUxpc3Rpbmc6NDU0OTAzNDI=");
        assert_eq!(variables["dateRange"]["startDate"], "2027-06-12");
        assert_eq!(variables["dateRange"]["endDate"], "2027-06-19");
        assert_eq!(variables["guestCounts"]["numberOfAdults"], 2);
        assert_eq!(variables["includePdpMigrationBookItSidebarFragment"], true);
        assert_eq!(variables["includeStaysPdpPriceHeatmapFragment"], false);
        assert!(variables["priceHeatmapDateRange"].is_null());
    }

    #[test]
    fn bookable_stay_of_2026_11_yields_total_average_and_breakdown() {
        let json = fixture_2026_11("StaysPdpBookItQuery.response.json");
        let quote = parse_quote_response(&json, &request(), 7, "$").unwrap();
        assert!(quote.available);
        assert_eq!(quote.instant_book, Some(true));
        assert_eq!(quote.total_display.as_deref(), Some("$1,400"));
        assert_eq!(quote.total, Some(1400.0));
        assert_eq!(quote.nightly, Some(200.0));
        assert_eq!(quote.listed_nightly, Some(200.0));
        assert_eq!(quote.currency, "$");
        assert_eq!(quote.nights, 7);
        assert_eq!(
            quote.breakdown,
            [
                PriceLine {
                    description: "7 nights x $200.00".into(),
                    amount: "$1,400.00".into()
                },
                PriceLine {
                    description: "Total".into(),
                    amount: "$1,400.00".into()
                },
            ]
        );
        assert_eq!(
            quote.notes,
            [
                "Pay $0 today with no extra fees.",
                "$0 today \u{b7} Free cancellation before May 13",
                "Rare find! This place is usually booked",
            ]
        );
        assert_eq!(quote.unavailability_message, None);
    }

    #[test]
    fn the_listed_nightly_leaves_fees_out_and_the_average_does_not() {
        // 7 nights at 200 plus a 120 cleaning fee: search results show 200,
        // the total divided by the nights gives 217.14.
        let json = serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
            "structuredDisplayPrice": {
                "primaryLine": {"price": "$1,520", "qualifier": "total"},
                "explanationData": {"priceDetails": [{"items": [
                    {"description": "7 nights x $200.00", "priceString": "$1,400.00"},
                    {"description": "Cleaning fee", "priceString": "$120.00"},
                    {"description": "Total", "priceString": "$1,520.00"}
                ]}]}
            },
            "availability": {"isAvailable": true}
        }}}}});
        let quote = parse_quote_response(&json, &request(), 7, "$").unwrap();
        assert_eq!(quote.total, Some(1520.0));
        assert_eq!(quote.listed_nightly, Some(200.0));
        assert!((quote.nightly.unwrap() - 1520.0 / 7.0).abs() < 1e-9);
        let text = quote.to_string();
        assert!(
            text.contains("Total: $1,520 ($217.14 per night on average, fees and taxes included)"),
            "{text}"
        );
        assert!(
            text.contains(
                "Nightly rate before fees and taxes: $200.00 (the price search results show)"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_zero_total_is_not_a_price() {
        let panel = |total: &str| {
            serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
                "structuredDisplayPrice": {
                    "primaryLine": {"price": total, "qualifier": "total"},
                    "explanationData": {"priceDetails": [{"items": [
                        {"description": "Total", "priceString": total}
                    ]}]}
                },
                "availability": {"isAvailable": false}
            }}}}})
        };
        let quote = parse_quote_response(&panel("$0"), &request(), 7, "$").unwrap();
        assert_eq!((quote.total, quote.nightly), (None, None));
        assert_eq!(quote.listed_nightly, None);
        assert_eq!(quote.total_display, None);
        // An unavailable stay can still carry a real price.
        let priced = parse_quote_response(&panel("$700"), &request(), 7, "$").unwrap();
        assert!(!priced.available);
        assert_eq!(priced.total, Some(700.0));
    }

    #[test]
    fn panel_notes_are_capped() {
        let details: Vec<Value> = (0..500)
            .map(|n| serde_json::json!({"content": format!("note {n}")}))
            .collect();
        let json = serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
            "structuredDisplayPrice": {
                "primaryLine": {"price": "$700", "qualifier": "total"},
                "explanationData": {"priceDetails": details}
            },
            "availability": {"isAvailable": true}
        }}}}});
        let quote = parse_quote_response(&json, &request(), 7, "$").unwrap();
        assert_eq!(quote.notes.len(), MAX_PANEL_NOTES);
    }

    #[test]
    fn unbookable_stay_is_a_quote_with_the_reason_and_no_price() {
        let quote =
            parse_quote_response(&unavailable("Minimum stay is 7 nights"), &request(), 1, "$")
                .unwrap();
        assert!(!quote.available);
        assert_eq!(
            quote.unavailability_message.as_deref(),
            Some("Minimum stay is 7 nights")
        );
        assert_eq!(quote.total, None);
        assert_eq!(quote.total_display, None);
        assert_eq!(quote.nightly, None);
        assert_eq!(quote.currency, "$");
        assert_eq!(quote.breakdown, Vec::<PriceLine>::new());
        assert_eq!(quote.notes, Vec::<String>::new());
    }

    #[test]
    fn nightly_primary_line_is_multiplied_into_a_total() {
        let json = serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
            "structuredDisplayPrice": {
                "primaryLine": {"price": "\u{20ac}120", "qualifier": "night"},
                "explanationData": {"priceDetails": [
                    {"items": [{"description": "Cleaning fee", "priceString": "\u{20ac}60"}]}
                ]}
            },
            "availability": {"isAvailable": true, "canInstantBook": false}
        }}}}});
        let quote = parse_quote_response(&json, &request(), 3, "$").unwrap();
        assert_eq!(quote.total, Some(360.0));
        assert_eq!(quote.total_display.as_deref(), Some("\u{20ac}360.00"));
        assert_eq!(quote.nightly, Some(120.0));
        assert_eq!(quote.listed_nightly, Some(120.0));
        assert_eq!(quote.currency, "\u{20ac}");
        assert_eq!(quote.instant_book, Some(false));
        assert_eq!(
            quote.breakdown.len(),
            1,
            "falls back to the display breakdown"
        );
    }

    #[test]
    fn drift_is_an_upstream_schema_error() {
        let cases = [
            (
                serde_json::json!({"data": {"node": null}}),
                "no data.node.pdpPresentation.bookIt",
            ),
            (
                serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {"availability": {}}}}}}),
                "no availability.isAvailable",
            ),
            (
                serde_json::json!({"data": {"node": {"pdpPresentation": {"bookIt": {
                    "availability": {"isAvailable": true}, "structuredDisplayPrice": null
                }}}}}),
                "bookable stay came without a price",
            ),
        ];
        for (json, expected) in cases {
            let err = parse_quote_response(&json, &request(), 7, "$").unwrap_err();
            assert!(
                matches!(err, AirbnbError::UpstreamSchema { ref operation, .. } if operation == BOOK_IT_OPERATION),
                "{err}"
            );
            assert!(err.to_string().contains(expected), "{expected}: {err}");
        }
    }
}
