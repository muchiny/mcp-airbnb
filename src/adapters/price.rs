//! Locale-aware parsing of Airbnb display prices and currency labels.
//!
//! Airbnb renders amounts as display strings whose format depends on the
//! currency and the locale (`$2,245`, `1 234,56 €`, `€ 45.14`, `1.234 €`).
//! Every parser turns such strings into numbers through this module, so the
//! separator rules live in one place.

use serde_json::Value;

use crate::domain::listing::SearchResult;

/// Characters Airbnb prints between thousands groups, besides `,` and `.`.
const GROUP_SPACES: &[char] = &[' ', '\u{a0}', '\u{202f}', '\u{2009}', '\'', '\u{2019}'];

/// Currency symbols that can prefix or suffix an amount.
const SYMBOL_CHARS: &[char] = &[
    '$', '\u{20ac}', '\u{a3}', '\u{a5}', '\u{20b9}', '\u{20a9}', '\u{20ab}', '\u{20ba}',
    '\u{20aa}', '\u{20b1}', '\u{e3f}', '\u{20b4}', '\u{20a6}', '\u{20a1}', '\u{20b2}', '\u{20b5}',
    '\u{20b8}', '\u{20bc}', '\u{20be}', '\u{20bd}', '\u{20ad}', '\u{20ae}', '\u{20a8}', '\u{fdfc}',
];

/// Alphabetic currency labels printed next to amounts.
const WORD_SYMBOLS: &[&str] = &[
    "kr", "z\u{142}", "K\u{10d}", "Ft", "lei", "Rp", "RM", "CHF", "R",
];

/// Words that mark a number of nights in price strings.
pub(crate) const NIGHT_WORDS: &[&str] = &[
    "night",
    "nuit",
    "noche",
    "notte",
    "nacht",
    "n\u{e4}chte",
    "noite",
];

/// A number found in a display string: char range and raw token.
struct NumberSpan {
    start: usize,
    end: usize,
    token: String,
}

/// Every number of `chars`, with thousands-group spaces folded in.
fn number_spans(chars: &[char]) -> Vec<NumberSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut token = String::new();
        while i < chars.len() {
            let c = chars[i];
            let next_is_digit = chars.get(i + 1).is_some_and(char::is_ascii_digit);
            if c.is_ascii_digit() || ((c == ',' || c == '.') && next_is_digit) {
                token.push(c);
            } else if !(GROUP_SPACES.contains(&c) && starts_three_digit_group(chars, i + 1)) {
                break;
            }
            i += 1;
        }
        spans.push(NumberSpan {
            start,
            end: i,
            token,
        });
    }
    spans
}

/// `true` when `chars[from..]` starts with exactly three digits.
fn starts_three_digit_group(chars: &[char], from: usize) -> bool {
    chars
        .get(from..from + 3)
        .is_some_and(|group| group.iter().all(char::is_ascii_digit))
        && !chars.get(from + 3).is_some_and(char::is_ascii_digit)
}

/// Turn a raw token (`"1,234.56"`, `"1.234,56"`, `"45,14"`) into a number.
fn normalize_number_token(token: &str) -> Option<f64> {
    let commas = token.matches(',').count();
    let dots = token.matches('.').count();
    let normalized = if commas > 0 && dots > 0 {
        // Both separators: the last one is the decimal separator.
        let decimal = if token.rfind(',') > token.rfind('.') {
            ','
        } else {
            '.'
        };
        let group = if decimal == ',' { '.' } else { ',' };
        if token.matches(decimal).count() != 1 {
            return None;
        }
        token.replace(group, "").replace(decimal, ".")
    } else if commas + dots == 0 {
        token.to_string()
    } else {
        let separator = if commas > 0 { ',' } else { '.' };
        let groups: Vec<&str> = token.split(separator).collect();
        match groups.as_slice() {
            [int_part, fraction] => match fraction.len() {
                1 | 2 => format!("{int_part}.{fraction}"),
                3 if *int_part == "0" => format!("0.{fraction}"),
                // "1,234" and "1.234": one separator before exactly three
                // digits is a thousands separator in every Airbnb locale.
                3 if int_part.len() <= 3 => format!("{int_part}{fraction}"),
                _ => return None,
            },
            [_, rest @ ..] if rest.iter().all(|group| group.len() == 3) => groups.concat(),
            _ => return None,
        }
    };
    normalized
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// Parse the first number of a display price string.
///
/// Thousands separators (`,` `.` space, no-break spaces, apostrophes) are
/// removed; the decimal separator is the last `,` or `.` followed by one or
/// two digits. A single separator followed by exactly three digits is a
/// thousands separator (`1,234` and `1.234` are both 1234). Returns `None`
/// when the string has no number or its separators are ambiguous.
pub fn parse_price_amount(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().collect();
    let span = number_spans(&chars).into_iter().next()?;
    normalize_number_token(&span.token)
}

/// Currency label printed next to the first number of `s`, normalized with
/// [`normalize_currency`] (`"85 €"` → `"€"`, `"450 EUR"` → `"€"`).
pub fn detect_currency(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let span = number_spans(&chars).into_iter().next()?;
    currency_around(&chars, &span)
}

/// Currency label directly before or after a number.
fn currency_around(chars: &[char], span: &NumberSpan) -> Option<String> {
    let before: String = chars[..span.start].iter().collect();
    let after: String = chars[span.end..].iter().collect();
    let prefix = before
        .split_whitespace()
        .last()
        .map(|word| word.trim_start_matches(['-', '+', '(']));
    let suffix = after
        .split_whitespace()
        .next()
        .map(|word| word.trim_end_matches([')', ',', '.', ';']));
    prefix
        .into_iter()
        .chain(suffix)
        .find(|word| is_currency_token(word))
        .map(normalize_currency)
}

fn is_currency_token(word: &str) -> bool {
    !word.is_empty()
        && word.chars().count() <= 4
        && (word.chars().any(|c| SYMBOL_CHARS.contains(&c))
            || WORD_SYMBOLS.contains(&word)
            || (word.len() == 3 && word.chars().all(|c| c.is_ascii_uppercase())))
}

/// Normalize a currency label: ISO 4217 codes become the symbol Airbnb
/// displays (`"USD"` → `"$"`, `"EUR"` → `"€"`); other labels are trimmed.
pub fn normalize_currency(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() == 3 && trimmed.chars().all(|c| c.is_ascii_uppercase()) {
        currency_symbol(trimmed)
    } else {
        trimmed.to_string()
    }
}

/// Symbol Airbnb displays for an ISO 4217 code (`"EUR"` → `"€"`). Codes
/// without a well-known symbol are returned upper-cased.
pub fn currency_symbol(iso: &str) -> String {
    let code = iso.trim().to_ascii_uppercase();
    let symbol = match code.as_str() {
        "USD" => "$",
        "EUR" => "\u{20ac}",
        "GBP" => "\u{a3}",
        "JPY" => "\u{a5}",
        "CNY" => "CN\u{a5}",
        "INR" => "\u{20b9}",
        "KRW" => "\u{20a9}",
        "VND" => "\u{20ab}",
        "TRY" => "\u{20ba}",
        "ILS" => "\u{20aa}",
        "PHP" => "\u{20b1}",
        "THB" => "\u{e3f}",
        "UAH" => "\u{20b4}",
        "NGN" => "\u{20a6}",
        "BRL" => "R$",
        "CAD" => "CA$",
        "AUD" => "A$",
        "NZD" => "NZ$",
        "MXN" => "MX$",
        "HKD" => "HK$",
        "SGD" => "S$",
        "TWD" => "NT$",
        _ => return code,
    };
    symbol.to_string()
}

/// Find a price inside free text such as `"€107 night"`, `"107 € par nuit"`
/// or `"Total: $1,234.50"`.
///
/// Only a number next to a currency label is a price: a bare number, even
/// one followed by a "night" word, may be a night count (`"2 nights
/// minimum"`). Returns the amount and the currency label printed with it.
pub fn find_price_in_text(s: &str) -> Option<(f64, Option<String>)> {
    let chars: Vec<char> = s.chars().collect();
    number_spans(&chars).iter().find_map(|span| {
        let currency = currency_around(&chars, span)?;
        normalize_number_token(&span.token)
            .filter(|amount| *amount > 0.0)
            .map(|amount| (amount, Some(currency)))
    })
}
/// Price information decoded from an Airbnb `structuredDisplayPrice` block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayPrice {
    /// Nightly price, when Airbnb shows one or it can be derived from the stay.
    pub nightly: Option<f64>,
    /// Stay total shown by Airbnb (never the struck-through `originalPrice`).
    pub total: Option<f64>,
    /// Currency label printed next to the amount (see [`normalize_currency`]).
    pub currency: Option<String>,
}

/// One `"N nights x price"` line of a price breakdown.
#[derive(Debug, Clone, PartialEq)]
pub struct NightlyBreakdown {
    /// Number of nights on the line.
    pub nights: u32,
    /// Price of one night.
    pub nightly: f64,
    /// Currency label printed next to the nightly price.
    pub currency: Option<String>,
}

/// What the `qualifier` of a primary price line says the amount covers.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Qualifier {
    /// No qualifier: legacy payloads print a bare nightly price.
    Unqualified,
    /// One night ("night", "per night").
    Night,
    /// The whole stay ("total", "for 5 nights").
    Total { nights: Option<u32> },
    /// A unit that is not a night ("month", "week").
    Other,
}

/// Decode a `structuredDisplayPrice` (search result or booking sidebar).
///
/// Nightly price priority: the `"N nights x price"` breakdown line; the
/// primary amount when its qualifier is a night or absent; the primary total
/// divided by the number of nights given by the qualifier or `nights_hint`
/// (a zero hint is ignored; taxes included, so an approximation); otherwise
/// unknown. The total comes
/// from a total-qualified primary or secondary line or the breakdown's
/// `Total` line, never from `originalPrice`.
pub fn parse_structured_display_price(sdp: &Value, nights_hint: Option<u32>) -> DisplayPrice {
    let primary = sdp.get("primaryLine");
    let primary_price = primary.and_then(primary_price_str);
    let qualifier = primary
        .and_then(primary_qualifier)
        .map_or(Qualifier::Unqualified, classify_qualifier);
    let primary_amount = primary_price
        .and_then(parse_price_amount)
        .filter(|amount| *amount > 0.0);
    let breakdown = explanation_descriptions(sdp).find_map(parse_nightly_breakdown);

    let currency = primary_price
        .and_then(detect_currency)
        .or_else(|| breakdown.as_ref().and_then(|b| b.currency.clone()));
    let total = match qualifier {
        Qualifier::Total { .. } => primary_amount,
        _ => secondary_total(sdp).or_else(|| explanation_total(sdp)),
    };
    let nightly = breakdown.map(|b| b.nightly).or_else(|| match qualifier {
        Qualifier::Unqualified | Qualifier::Night => primary_amount,
        Qualifier::Total { nights } => primary_amount
            .zip(nights.or(nights_hint.filter(|n| *n > 0)))
            .map(|(stay_total, n)| stay_total / f64::from(n)),
        Qualifier::Other => None,
    });
    DisplayPrice {
        nightly,
        total,
        currency,
    }
}

/// Parse a breakdown line such as `"5 nights x $446.94"`,
/// `"€ 45.14 x 5 nights"` or `"5 nuits x 45,14 €"`. Lines that count months
/// or weeks return `None`.
pub fn parse_nightly_breakdown(description: &str) -> Option<NightlyBreakdown> {
    let (left, right) = [" x ", " \u{d7} ", " X "]
        .iter()
        .find_map(|separator| description.split_once(*separator))?;
    let (nights, price_side) = match (night_count(left), night_count(right)) {
        (Some(n), _) => (n, right),
        (None, Some(n)) => (n, left),
        (None, None) => return None,
    };
    let nightly = parse_price_amount(price_side).filter(|p| *p > 0.0)?;
    Some(NightlyBreakdown {
        nights,
        nightly,
        currency: detect_currency(price_side),
    })
}

fn night_count(side: &str) -> Option<u32> {
    let lower = side.to_lowercase();
    if !NIGHT_WORDS.iter().any(|word| lower.contains(word)) {
        return None;
    }
    lower
        .split_whitespace()
        .find_map(|word| word.parse::<u32>().ok())
        .filter(|n| *n > 0)
}

fn classify_qualifier(qualifier: &str) -> Qualifier {
    let lower = qualifier.trim().to_lowercase();
    if lower.is_empty() {
        return Qualifier::Unqualified;
    }
    let mentions_nights = NIGHT_WORDS.iter().any(|word| lower.contains(word));
    let count = lower
        .split_whitespace()
        .find_map(|word| word.parse::<u32>().ok())
        .filter(|n| *n > 0);
    if lower.contains("total") || (mentions_nights && count.is_some()) {
        Qualifier::Total {
            nights: count.filter(|_| mentions_nights),
        }
    } else if mentions_nights {
        Qualifier::Night
    } else {
        Qualifier::Other
    }
}

/// Amount string of a primary line: `discountedPrice`, `price`, the same
/// keys inside `orderedComponents`, then `originalPrice` as a last resort.
fn primary_price_str(primary: &Value) -> Option<&str> {
    for key in ["discountedPrice", "price"] {
        if let Some(value) = primary.get(key).and_then(Value::as_str) {
            return Some(value);
        }
    }
    if let Some(components) = primary.get("orderedComponents").and_then(Value::as_array) {
        for key in ["discountedPrice", "price"] {
            if let Some(value) = components
                .iter()
                .find_map(|c| c.get(key).and_then(Value::as_str))
            {
                return Some(value);
            }
        }
    }
    primary.get("originalPrice").and_then(Value::as_str)
}

fn primary_qualifier(primary: &Value) -> Option<&str> {
    primary
        .get("qualifier")
        .and_then(Value::as_str)
        .or_else(|| {
            primary
                .get("orderedComponents")?
                .as_array()?
                .iter()
                .find_map(|component| component.get("qualifier").and_then(Value::as_str))
        })
}

fn explanation_items(sdp: &Value) -> impl Iterator<Item = &Value> {
    sdp.pointer("/explanationData/priceDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("items").and_then(Value::as_array))
        .flatten()
}

fn explanation_descriptions(sdp: &Value) -> impl Iterator<Item = &str> {
    explanation_items(sdp).filter_map(|item| item.get("description").and_then(Value::as_str))
}

fn secondary_total(sdp: &Value) -> Option<f64> {
    let line = sdp.get("secondaryLine")?;
    let qualifier = line.get("qualifier").and_then(Value::as_str)?;
    if !matches!(classify_qualifier(qualifier), Qualifier::Total { .. }) {
        return None;
    }
    line.get("price")
        .and_then(Value::as_str)
        .and_then(parse_price_amount)
}

fn explanation_total(sdp: &Value) -> Option<f64> {
    explanation_items(sdp)
        .find(|item| {
            item.get("description")
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim().eq_ignore_ascii_case("total"))
        })
        .and_then(|item| item.get("priceString").and_then(Value::as_str))
        .and_then(parse_price_amount)
}

/// Label an unknown currency (empty string) with the pinned currency symbol.
///
/// Every request pins `currency=` (`ScraperConfig::currency`), so an amount
/// printed without a symbol is in the pinned currency. A label read from the
/// payload is kept even when it differs.
pub fn fill_currency(currency: &mut String, pinned_symbol: &str) {
    if currency.trim().is_empty() {
        *currency = pinned_symbol.to_string();
    }
}

/// [`fill_currency`] for every listing of a search result.
pub fn fill_search_currency(result: &mut SearchResult, pinned_symbol: &str) {
    for listing in &mut result.listings {
        fill_currency(&mut listing.currency, pinned_symbol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locale_formats_from_the_audit() {
        let cases: [(&str, Option<f64>); 16] = [
            ("$2,245", Some(2245.0)),
            ("$446.94", Some(446.94)),
            ("$1,234", Some(1234.0)),
            ("45,14 \u{20ac}", Some(45.14)),
            ("1 234,56 \u{20ac}", Some(1234.56)),
            ("1\u{202f}234,56\u{a0}\u{20ac}", Some(1234.56)),
            ("1.234,56 \u{20ac}", Some(1234.56)),
            ("\u{20ac}1.234", Some(1234.0)),
            ("1.234 \u{20ac}", Some(1234.0)),
            ("1.234.567 \u{20ab}", Some(1_234_567.0)),
            ("\u{20ac} 450 total \u{b7} 5 nights", Some(450.0)),
            ("\u{a5}12000", Some(12_000.0)),
            ("CHF 1'234.50", Some(1234.5)),
            ("107,50 \u{20ac}", Some(107.5)),
            ("-$34.70", Some(34.7)),
            ("Contact host", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_price_amount(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn rejects_ambiguous_separators() {
        assert_eq!(parse_price_amount("1234.567"), None);
        assert_eq!(parse_price_amount("1,23,45"), None);
        assert_eq!(parse_price_amount(""), None);
    }

    #[test]
    fn detects_prefix_and_suffix_currency_labels() {
        assert_eq!(detect_currency("$2,245").as_deref(), Some("$"));
        assert_eq!(detect_currency("\u{20ac} 254").as_deref(), Some("\u{20ac}"));
        assert_eq!(detect_currency("85 \u{20ac}").as_deref(), Some("\u{20ac}"));
        assert_eq!(
            detect_currency("1\u{202f}234,56\u{a0}\u{20ac}").as_deref(),
            Some("\u{20ac}")
        );
        assert_eq!(detect_currency("CA$120").as_deref(), Some("CA$"));
        assert_eq!(detect_currency("450 EUR").as_deref(), Some("\u{20ac}"));
        assert_eq!(detect_currency("-$34.70").as_deref(), Some("$"));
        assert_eq!(detect_currency("2,245 total"), None);
        assert_eq!(detect_currency("120"), None);
    }

    #[test]
    fn normalizes_iso_codes_to_display_symbols() {
        assert_eq!(normalize_currency("USD"), "$");
        assert_eq!(normalize_currency(" EUR "), "\u{20ac}");
        assert_eq!(normalize_currency("SEK"), "SEK");
        assert_eq!(normalize_currency("\u{20ac}"), "\u{20ac}");
        assert_eq!(normalize_currency("lei"), "lei");
        assert_eq!(currency_symbol("eur"), "\u{20ac}");
        assert_eq!(currency_symbol("CAD"), "CA$");
    }

    #[test]
    fn finds_prices_inside_free_text() {
        assert_eq!(
            find_price_in_text("$120"),
            Some((120.0, Some("$".to_string())))
        );
        assert_eq!(
            find_price_in_text("\u{20ac}95.50 night"),
            Some((95.5, Some("\u{20ac}".to_string())))
        );
        assert_eq!(
            find_price_in_text("\u{a3} 200 per night"),
            Some((200.0, Some("\u{a3}".to_string())))
        );
        assert_eq!(
            find_price_in_text("107 \u{20ac}"),
            Some((107.0, Some("\u{20ac}".to_string())))
        );
        assert_eq!(
            find_price_in_text("85\u{20ac}"),
            Some((85.0, Some("\u{20ac}".to_string())))
        );
        assert_eq!(
            find_price_in_text("1 234 \u{20ac} par nuit"),
            Some((1234.0, Some("\u{20ac}".to_string())))
        );
        // A number without a currency label is never a price: it may be a
        // night count ("2 nights minimum").
        assert_eq!(find_price_in_text("107 night"), None);
        assert_eq!(find_price_in_text("85 nuit"), None);
        assert_eq!(find_price_in_text("2 nights minimum"), None);
        assert_eq!(find_price_in_text("1 night minimum"), None);
        assert_eq!(
            find_price_in_text("5 nights x $446.94"),
            Some((446.94, Some("$".to_string())))
        );
        assert_eq!(find_price_in_text("Beautiful apartment"), None);
        assert_eq!(find_price_in_text("4.96 rating"), None);
    }

    fn group_thousands(int_part: u64, separator: &str) -> String {
        let digits = int_part.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push_str(separator);
            }
            out.push(c);
        }
        out
    }

    proptest::proptest! {
        #[test]
        fn round_trips_en_fr_de_formats(cents in 1u64..1_000_000_000) {
            let int_part = cents / 100;
            let frac = cents % 100;
            let expected = f64::from(u32::try_from(cents).expect("fits in u32")) / 100.0;
            let en = format!("${}.{frac:02}", group_thousands(int_part, ","));
            let fr = format!("{},{frac:02}\u{a0}\u{20ac}", group_thousands(int_part, "\u{202f}"));
            let de = format!("{},{frac:02} \u{20ac}", group_thousands(int_part, "."));
            for input in [en, fr, de] {
                let parsed = parse_price_amount(&input);
                proptest::prop_assert!(
                    parsed.is_some_and(|p| (p - expected).abs() < 1e-6),
                    "{input:?} -> {parsed:?}"
                );
            }
        }
    }

    use serde_json::{Value, json};

    #[test]
    fn nightly_price_comes_from_the_breakdown_not_the_total() {
        let search = crate::test_helpers::fixture_json("p1b/stays_search.json");
        let items = search
            .pointer("/data/presentation/staysSearch/results/searchResults")
            .and_then(Value::as_array)
            .expect("searchResults");
        // Item 3 is the discounted `orderedComponents` variant (no `price` key).
        let expected = [(446.94, 2245.0), (158.39, 797.0), (147.24, 810.0)];
        for (item, (nightly, total)) in items.iter().zip(expected) {
            let price = parse_structured_display_price(&item["structuredDisplayPrice"], Some(5));
            assert_eq!(price.nightly, Some(nightly));
            assert_eq!(price.total, Some(total));
            assert_eq!(price.currency.as_deref(), Some("$"));
        }
    }

    #[test]
    fn qualifier_decides_what_the_primary_amount_means() {
        let night = json!({"primaryLine": {"price": "\u{20ac} 85", "qualifier": "night"}});
        let p = parse_structured_display_price(&night, None);
        assert_eq!((p.nightly, p.total), (Some(85.0), None));

        let stay = json!({"primaryLine": {"price": "\u{20ac} 450", "qualifier": "for 5 nights"}});
        let p = parse_structured_display_price(&stay, None);
        assert_eq!((p.nightly, p.total), (Some(90.0), Some(450.0)));

        let total = json!({"primaryLine": {"price": "$750", "qualifier": "total"}});
        let p = parse_structured_display_price(&total, None);
        assert_eq!((p.nightly, p.total), (None, Some(750.0)));
        assert_eq!(
            parse_structured_display_price(&total, Some(5)).nightly,
            Some(150.0)
        );
        assert_eq!(
            parse_structured_display_price(&total, Some(0)).nightly,
            None
        );

        let monthly = json!({"primaryLine": {"price": "$2,345", "qualifier": "month"}});
        assert_eq!(parse_structured_display_price(&monthly, None).nightly, None);

        let legacy = json!({"primaryLine": {"price": "$150"}});
        assert_eq!(
            parse_structured_display_price(&legacy, None).nightly,
            Some(150.0)
        );
    }

    #[test]
    fn discounted_line_never_reports_the_original_price_as_total() {
        let sdp = json!({"primaryLine": {"discountedPrice": "$90", "originalPrice": "$120", "qualifier": "night"}});
        let p = parse_structured_display_price(&sdp, None);
        assert_eq!(p.nightly, Some(90.0));
        assert_eq!(p.total, None);
        assert_eq!(p.currency.as_deref(), Some("$"));
    }

    #[test]
    fn secondary_line_total_is_used_for_nightly_primary_lines() {
        let sdp = json!({
            "primaryLine": {"price": "$150", "qualifier": "night"},
            "secondaryLine": {"price": "$750", "qualifier": "total"}
        });
        let p = parse_structured_display_price(&sdp, None);
        assert_eq!((p.nightly, p.total), (Some(150.0), Some(750.0)));
    }

    #[test]
    fn breakdown_lines_in_every_order_and_unit() {
        assert_eq!(
            parse_nightly_breakdown("5 nights x $446.94"),
            Some(NightlyBreakdown {
                nights: 5,
                nightly: 446.94,
                currency: Some("$".into())
            })
        );
        assert_eq!(
            parse_nightly_breakdown("\u{20ac} 45.14 x 5 nights"),
            Some(NightlyBreakdown {
                nights: 5,
                nightly: 45.14,
                currency: Some("\u{20ac}".into())
            })
        );
        assert_eq!(
            parse_nightly_breakdown("5 nuits x 45,14 \u{20ac}"),
            Some(NightlyBreakdown {
                nights: 5,
                nightly: 45.14,
                currency: Some("\u{20ac}".into())
            })
        );
        assert_eq!(parse_nightly_breakdown("1 month x $2,345.00"), None);
        assert_eq!(parse_nightly_breakdown("Taxes"), None);
    }
}
