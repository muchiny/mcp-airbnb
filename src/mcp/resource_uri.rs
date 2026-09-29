//! Canonical `airbnb://` resource URIs and the templates that advertise them.
//!
//! Every variable part is percent-encoded, keeping only the RFC 3986
//! *unreserved* characters (`A-Z a-z 0-9 - . _ ~`) literal. That is exactly
//! what RFC 6570 produces when a client expands the advertised templates
//! (`{var}` simple expansion, `{?a,b}` form-style query). So a URI built
//! here and a URI a client builds from the template are byte-for-byte
//! equal. Such URIs also pass through URL parsers unchanged: pydantic's
//! `AnyUrl` (MCP Python SDK) keeps `airbnb://search/Paris%2C%20France` as
//! is, but it rewrote the raw `airbnb://search/Paris, France` the server
//! used to store (MCP-3).
//!
//! Every input that changes a tool's result is part of its URI (cursor,
//! dates, filters, months, `max_pages`), so a later call never overwrites
//! the resource of an earlier, different call.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::domain::search_params::SearchParams;

/// Bytes kept literal: the RFC 3986 unreserved set.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Characters that structure the URIs built here. [`encode`] never emits
/// them, so a literal one is always a delimiter.
const DELIMITERS: [char; 5] = ['/', '?', '&', '=', ','];

const SCHEME: &str = "airbnb://";

/// Longest URI the resource store keeps, in bytes. Inputs are capped well
/// below this: a location has at most 200 characters, a cursor at most
/// 1024, and a market comparison at most 5 locations.
pub const MAX_URI_LEN: usize = 16 * 1024;

/// One advertised resource template.
#[derive(Debug, Clone, Copy)]
pub struct TemplateSpec {
    /// RFC 6570 template.
    pub uri_template: &'static str,
    /// Short machine-friendly name.
    pub name: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// What the resource holds and which tool fills it.
    pub description: &'static str,
}

/// The 18 resource templates, one per tool.
pub const TEMPLATES: [TemplateSpec; 18] = [
    TemplateSpec {
        uri_template: "airbnb://listing/{id}",
        name: "Airbnb Listing",
        title: "Listing details",
        description: "Full listing details (from airbnb_listing_details)",
    },
    TemplateSpec {
        uri_template: "airbnb://listing/{id}/calendar{?months}",
        name: "Price Calendar",
        title: "Availability calendar",
        description: "Daily availability for `months` months (from airbnb_price_calendar)",
    },
    TemplateSpec {
        uri_template: "airbnb://listing/{id}/reviews{?cursor}",
        name: "Reviews",
        title: "Guest reviews",
        description: "One page of guest reviews; `cursor` selects the page (from airbnb_reviews)",
    },
    TemplateSpec {
        uri_template: "airbnb://listing/{id}/host",
        name: "Host Profile",
        title: "Host profile",
        description: "Host bio, superhost status, response rate (from airbnb_host_profile)",
    },
    TemplateSpec {
        uri_template: "airbnb://listing/{id}/occupancy{?months}",
        name: "Occupancy Estimate",
        title: "Occupancy estimate",
        description: "Occupancy over `months` months (from airbnb_occupancy_estimate)",
    },
    TemplateSpec {
        uri_template: "airbnb://search/{location}{?checkin,checkout,adults,children,infants,pets,min_price,max_price,property_type,cursor}",
        name: "Search Results",
        title: "Search results",
        description: "One page of search results for these exact filters (from airbnb_search)",
    },
    TemplateSpec {
        uri_template: "airbnb://neighborhood/{location}{?checkin,checkout,property_type}",
        name: "Neighborhood Stats",
        title: "Neighborhood statistics",
        description: "Area-level price and rating statistics (from airbnb_neighborhood_stats)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/compare{?ids,location,max_listings,checkin,checkout,property_type}",
        name: "Listing Comparison",
        title: "Listing comparison",
        description: "Comparison of `ids`, or of up to `max_listings` listings found in `location` (from airbnb_compare_listings)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/price-trends/{id}{?months}",
        name: "Price Trends",
        title: "Price trends",
        description: "Seasonal price analysis (from airbnb_price_trends)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/gaps/{id}{?months}",
        name: "Booking Gaps",
        title: "Booking gaps",
        description: "Orphan nights and booking gaps (from airbnb_gap_finder)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/revenue{?id,location,months}",
        name: "Revenue Estimate",
        title: "Revenue estimate",
        description: "Revenue projection for a listing `id` and/or a `location` (from airbnb_revenue_estimate)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/score/{id}",
        name: "Listing Score",
        title: "Listing score",
        description: "Quality audit 0-100 (from airbnb_listing_score)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/amenities/{id}{?location}",
        name: "Amenity Analysis",
        title: "Amenity analysis",
        description: "Amenities vs the neighborhood (from airbnb_amenity_analysis)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/market/{locations}{?checkin,checkout,property_type}",
        name: "Market Comparison",
        title: "Market comparison",
        description: "2-5 markets side by side; `locations` is a comma-separated list (from airbnb_market_comparison)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/portfolio/{id}",
        name: "Host Portfolio",
        title: "Host portfolio",
        description: "Listings by the host of listing `id` (from airbnb_host_portfolio)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/sentiment/{id}{?max_pages}",
        name: "Review Sentiment",
        title: "Review sentiment",
        description: "Sentiment over `max_pages` review pages (from airbnb_review_sentiment)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/positioning/{id}{?location}",
        name: "Competitive Positioning",
        title: "Competitive positioning",
        description: "5-axis position vs the neighborhood (from airbnb_competitive_positioning)",
    },
    TemplateSpec {
        uri_template: "airbnb://analysis/pricing/{id}{?location,months}",
        name: "Optimal Pricing",
        title: "Optimal pricing",
        description: "Pricing recommendation (from airbnb_optimal_pricing)",
    },
];

/// Percent-encode one variable value (RFC 6570 simple expansion).
pub fn encode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

/// Encode a list value the way RFC 6570 expands a list variable: each item
/// is encoded, and items are joined by a literal comma.
pub fn encode_list<S: AsRef<str>>(values: &[S]) -> String {
    values
        .iter()
        .map(|value| encode(value.as_ref()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Append an RFC 6570 form-style query (`{?a,b,c}`). Pairs whose value is
/// `None` are left out. Values must already be encoded.
fn with_query(mut uri: String, pairs: &[(&str, Option<String>)]) -> String {
    let mut separator = '?';
    for (name, value) in pairs {
        if let Some(value) = value {
            uri.push(separator);
            uri.push_str(name);
            uri.push('=');
            uri.push_str(value);
            separator = '&';
        }
    }
    uri
}

fn text(value: Option<&str>) -> Option<String> {
    value.map(encode)
}

fn number(value: Option<u32>) -> Option<String> {
    value.as_ref().map(ToString::to_string)
}

/// `airbnb://listing/{id}`
pub fn listing(id: &str) -> String {
    format!("{SCHEME}listing/{}", encode(id))
}

/// `airbnb://listing/{id}/calendar{?months}`
pub fn calendar(id: &str, months: u32) -> String {
    with_query(
        format!("{SCHEME}listing/{}/calendar", encode(id)),
        &[("months", Some(months.to_string()))],
    )
}

/// `airbnb://listing/{id}/reviews{?cursor}`
pub fn reviews(id: &str, cursor: Option<&str>) -> String {
    with_query(
        format!("{SCHEME}listing/{}/reviews", encode(id)),
        &[("cursor", text(cursor))],
    )
}

/// `airbnb://listing/{id}/host`
pub fn host(id: &str) -> String {
    format!("{SCHEME}listing/{}/host", encode(id))
}

/// `airbnb://listing/{id}/occupancy{?months}`
pub fn occupancy(id: &str, months: u32) -> String {
    with_query(
        format!("{SCHEME}listing/{}/occupancy", encode(id)),
        &[("months", Some(months.to_string()))],
    )
}

/// `airbnb://search/{location}{?checkin,checkout,adults,children,infants,pets,min_price,max_price,property_type,cursor}`
pub fn search(params: &SearchParams) -> String {
    with_query(
        format!("{SCHEME}search/{}", encode(&params.location)),
        &[
            ("checkin", text(params.checkin.as_deref())),
            ("checkout", text(params.checkout.as_deref())),
            ("adults", number(params.adults)),
            ("children", number(params.children)),
            ("infants", number(params.infants)),
            ("pets", number(params.pets)),
            ("min_price", number(params.min_price)),
            ("max_price", number(params.max_price)),
            ("property_type", text(params.property_type.as_deref())),
            ("cursor", text(params.cursor.as_deref())),
        ],
    )
}

/// `airbnb://neighborhood/{location}{?checkin,checkout,property_type}`
pub fn neighborhood(params: &SearchParams) -> String {
    with_query(
        format!("{SCHEME}neighborhood/{}", encode(&params.location)),
        &[
            ("checkin", text(params.checkin.as_deref())),
            ("checkout", text(params.checkout.as_deref())),
            ("property_type", text(params.property_type.as_deref())),
        ],
    )
}

/// `airbnb://analysis/compare{?ids,…}` for a comparison of explicit ids.
pub fn compare_ids<S: AsRef<str>>(ids: &[S]) -> String {
    with_query(
        format!("{SCHEME}analysis/compare"),
        &[("ids", Some(encode_list(ids)))],
    )
}

/// `airbnb://analysis/compare{?…,location,max_listings,checkin,checkout,property_type}`
/// for a comparison of listings found by a location search.
pub fn compare_location(
    location: &str,
    max_listings: u32,
    checkin: Option<&str>,
    checkout: Option<&str>,
    property_type: Option<&str>,
) -> String {
    with_query(
        format!("{SCHEME}analysis/compare"),
        &[
            ("location", Some(encode(location))),
            ("max_listings", Some(max_listings.to_string())),
            ("checkin", text(checkin)),
            ("checkout", text(checkout)),
            ("property_type", text(property_type)),
        ],
    )
}

/// `airbnb://analysis/price-trends/{id}{?months}`
pub fn price_trends(id: &str, months: u32) -> String {
    with_query(
        format!("{SCHEME}analysis/price-trends/{}", encode(id)),
        &[("months", Some(months.to_string()))],
    )
}

/// `airbnb://analysis/gaps/{id}{?months}`
pub fn gaps(id: &str, months: u32) -> String {
    with_query(
        format!("{SCHEME}analysis/gaps/{}", encode(id)),
        &[("months", Some(months.to_string()))],
    )
}

/// `airbnb://analysis/revenue{?id,location,months}`. Named parameters, so a
/// listing id and a location that look alike never collide.
pub fn revenue(id: Option<&str>, location: Option<&str>, months: u32) -> String {
    with_query(
        format!("{SCHEME}analysis/revenue"),
        &[
            ("id", text(id)),
            ("location", text(location)),
            ("months", Some(months.to_string())),
        ],
    )
}

/// `airbnb://analysis/score/{id}`
pub fn score(id: &str) -> String {
    format!("{SCHEME}analysis/score/{}", encode(id))
}

/// `airbnb://analysis/amenities/{id}{?location}`
pub fn amenities(id: &str, location: Option<&str>) -> String {
    with_query(
        format!("{SCHEME}analysis/amenities/{}", encode(id)),
        &[("location", text(location))],
    )
}

/// `airbnb://analysis/market/{locations}{?checkin,checkout,property_type}`
pub fn market<S: AsRef<str>>(
    locations: &[S],
    checkin: Option<&str>,
    checkout: Option<&str>,
    property_type: Option<&str>,
) -> String {
    with_query(
        format!("{SCHEME}analysis/market/{}", encode_list(locations)),
        &[
            ("checkin", text(checkin)),
            ("checkout", text(checkout)),
            ("property_type", text(property_type)),
        ],
    )
}

/// `airbnb://analysis/portfolio/{id}`
pub fn portfolio(id: &str) -> String {
    format!("{SCHEME}analysis/portfolio/{}", encode(id))
}

/// `airbnb://analysis/sentiment/{id}{?max_pages}`
pub fn sentiment(id: &str, max_pages: u32) -> String {
    with_query(
        format!("{SCHEME}analysis/sentiment/{}", encode(id)),
        &[("max_pages", Some(max_pages.to_string()))],
    )
}

/// `airbnb://analysis/positioning/{id}{?location}`
pub fn positioning(id: &str, location: Option<&str>) -> String {
    with_query(
        format!("{SCHEME}analysis/positioning/{}", encode(id)),
        &[("location", text(location))],
    )
}

/// `airbnb://analysis/pricing/{id}{?location,months}`
pub fn pricing(id: &str, location: Option<&str>, months: u32) -> String {
    with_query(
        format!("{SCHEME}analysis/pricing/{}", encode(id)),
        &[
            ("location", text(location)),
            ("months", Some(months.to_string())),
        ],
    )
}

/// Normalise a URI received in `resources/read` before the lookup.
///
/// Each run of characters between delimiters is percent-decoded and
/// re-encoded with [`encode`]. That fixes lower-case escapes (`%2c`),
/// escaped unreserved characters (`%7E`), raw spaces and raw non-ASCII text
/// (`São` becomes `S%C3%A3o`). Literal delimiters stay as they are: a
/// literal `,` is a list separator and an encoded `%2C` is part of a value,
/// and the two must stay distinct.
pub fn canonicalize(uri: &str) -> String {
    let Some(rest) = uri.strip_prefix(SCHEME) else {
        return uri.to_string();
    };
    let mut out = String::with_capacity(uri.len());
    out.push_str(SCHEME);
    let mut run = String::new();
    for c in rest.chars() {
        if DELIMITERS.contains(&c) {
            push_canonical(&mut out, &run);
            run.clear();
            out.push(c);
        } else {
            run.push(c);
        }
    }
    push_canonical(&mut out, &run);
    out
}

fn push_canonical(out: &mut String, run: &str) {
    if !run.is_empty() {
        let decoded = percent_decode_str(run).decode_utf8_lossy();
        out.push_str(&encode(&decoded));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent RFC 3986 encoder: expected values must not come from the code under test.
    fn rfc_encode(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    enum Var<'a> {
        One(&'a str),
        List(&'a [&'a str]),
    }

    fn render(value: &Var<'_>) -> String {
        match value {
            Var::One(v) => rfc_encode(v),
            Var::List(items) => {
                let mut parts = Vec::new();
                for item in *items {
                    parts.push(rfc_encode(item));
                }
                parts.join(",")
            }
        }
    }

    /// Minimal RFC 6570 expander for the two operators the templates use:
    /// `{var}` (simple) and `{?a,b}` (form-style query).
    fn expand(template: &str, vars: &[(&str, Var<'_>)]) -> String {
        let lookup = |name: &str| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| render(v))
        };
        let mut out = String::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let end = start + rest[start..].find('}').expect("closing brace");
            let expression = &rest[start + 1..end];
            if let Some(names) = expression.strip_prefix('?') {
                let mut separator = '?';
                for name in names.split(',') {
                    if let Some(value) = lookup(name) {
                        out.push(separator);
                        out.push_str(name);
                        out.push('=');
                        out.push_str(&value);
                        separator = '&';
                    }
                }
            } else if let Some(value) = lookup(expression) {
                out.push_str(&value);
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn template(name: &str) -> &'static str {
        TEMPLATES
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.uri_template)
            .expect("template exists")
    }

    #[test]
    fn builders_match_rfc6570_expansion_of_templates() {
        let params = SearchParams {
            location: "Paris, France".into(),
            checkin: Some("2026-10-01".into()),
            checkout: Some("2026-10-05".into()),
            adults: Some(2),
            property_type: Some("Entire home".into()),
            cursor: Some("abc/=+&".into()),
            ..SearchParams::default()
        };
        assert_eq!(
            search(&params),
            expand(
                template("Search Results"),
                &[
                    ("location", Var::One("Paris, France")),
                    ("checkin", Var::One("2026-10-01")),
                    ("checkout", Var::One("2026-10-05")),
                    ("adults", Var::One("2")),
                    ("property_type", Var::One("Entire home")),
                    ("cursor", Var::One("abc/=+&")),
                ],
            )
        );
        assert_eq!(
            market(&["Paris, France", "São Paulo"], None, None, None),
            expand(
                template("Market Comparison"),
                &[("locations", Var::List(&["Paris, France", "São Paulo"]))],
            )
        );
        assert_eq!(
            compare_ids(&["101", "102"]),
            expand(
                template("Listing Comparison"),
                &[("ids", Var::List(&["101", "102"]))]
            )
        );
        assert_eq!(
            compare_location(
                "Rome, Italy",
                40,
                Some("2026-10-01"),
                Some("2026-10-05"),
                None
            ),
            expand(
                template("Listing Comparison"),
                &[
                    ("location", Var::One("Rome, Italy")),
                    ("max_listings", Var::One("40")),
                    ("checkin", Var::One("2026-10-01")),
                    ("checkout", Var::One("2026-10-05")),
                ],
            )
        );
        assert_eq!(
            reviews("42", Some("24")),
            expand(
                template("Reviews"),
                &[("id", Var::One("42")), ("cursor", Var::One("24"))]
            )
        );
        assert_eq!(
            calendar("42", 3),
            expand(
                template("Price Calendar"),
                &[("id", Var::One("42")), ("months", Var::One("3"))]
            )
        );
        assert_eq!(
            revenue(None, Some("Lyon, France"), 12),
            expand(
                template("Revenue Estimate"),
                &[
                    ("location", Var::One("Lyon, France")),
                    ("months", Var::One("12"))
                ],
            )
        );
        assert_eq!(
            pricing("42", Some("Lyon"), 6),
            expand(
                template("Optimal Pricing"),
                &[
                    ("id", Var::One("42")),
                    ("location", Var::One("Lyon")),
                    ("months", Var::One("6")),
                ],
            )
        );
        assert_eq!(
            sentiment("42", 5),
            expand(
                template("Review Sentiment"),
                &[("id", Var::One("42")), ("max_pages", Var::One("5"))],
            )
        );
    }

    /// Built URI, template name, and the variables to expand it with.
    type Case<'a> = (String, &'a str, Vec<(&'a str, Var<'a>)>);

    fn with_dates<'a>(mut vars: Vec<(&'a str, Var<'a>)>) -> Vec<(&'a str, Var<'a>)> {
        vars.push(("checkin", Var::One("2026-10-01")));
        vars.push(("checkout", Var::One("2026-10-05")));
        vars
    }

    /// Every builder, with every variable its template names, against the
    /// independent expansion of that template (MCP-3). The id holds reserved
    /// characters so path encoding is checked too.
    #[test]
    #[allow(clippy::too_many_lines)] // one table row per builder
    fn every_builder_and_variable_matches_its_template_expansion() {
        const ID: &str = "4 2/x";
        const PLACE: &str = "São Paulo, Brasil";
        let full = SearchParams {
            location: PLACE.into(),
            checkin: Some("2026-10-01".into()),
            checkout: Some("2026-10-05".into()),
            adults: Some(2),
            children: Some(1),
            infants: Some(3),
            pets: Some(4),
            min_price: Some(50),
            max_price: Some(300),
            property_type: Some("Private room".into()),
            cursor: Some("c?u&r=s,o r".into()),
        };
        let cases: Vec<Case<'_>> = vec![
            (listing(ID), "Airbnb Listing", vec![("id", Var::One(ID))]),
            (
                calendar(ID, 12),
                "Price Calendar",
                vec![("id", Var::One(ID)), ("months", Var::One("12"))],
            ),
            (reviews(ID, None), "Reviews", vec![("id", Var::One(ID))]),
            (host(ID), "Host Profile", vec![("id", Var::One(ID))]),
            (
                occupancy(ID, 6),
                "Occupancy Estimate",
                vec![("id", Var::One(ID)), ("months", Var::One("6"))],
            ),
            (
                search(&full),
                "Search Results",
                with_dates(vec![
                    ("location", Var::One(PLACE)),
                    ("adults", Var::One("2")),
                    ("children", Var::One("1")),
                    ("infants", Var::One("3")),
                    ("pets", Var::One("4")),
                    ("min_price", Var::One("50")),
                    ("max_price", Var::One("300")),
                    ("property_type", Var::One("Private room")),
                    ("cursor", Var::One("c?u&r=s,o r")),
                ]),
            ),
            (
                neighborhood(&full),
                "Neighborhood Stats",
                with_dates(vec![
                    ("location", Var::One(PLACE)),
                    ("property_type", Var::One("Private room")),
                ]),
            ),
            (
                compare_ids(&["7", "8 9"]),
                "Listing Comparison",
                vec![("ids", Var::List(&["7", "8 9"]))],
            ),
            (
                compare_location(
                    PLACE,
                    100,
                    Some("2026-10-01"),
                    Some("2026-10-05"),
                    Some("Entire home"),
                ),
                "Listing Comparison",
                with_dates(vec![
                    ("location", Var::One(PLACE)),
                    ("max_listings", Var::One("100")),
                    ("property_type", Var::One("Entire home")),
                ]),
            ),
            (
                price_trends(ID, 3),
                "Price Trends",
                vec![("id", Var::One(ID)), ("months", Var::One("3"))],
            ),
            (
                gaps(ID, 2),
                "Booking Gaps",
                vec![("id", Var::One(ID)), ("months", Var::One("2"))],
            ),
            (
                revenue(Some(ID), Some(PLACE), 24),
                "Revenue Estimate",
                vec![
                    ("id", Var::One(ID)),
                    ("location", Var::One(PLACE)),
                    ("months", Var::One("24")),
                ],
            ),
            (score(ID), "Listing Score", vec![("id", Var::One(ID))]),
            (
                amenities(ID, Some(PLACE)),
                "Amenity Analysis",
                vec![("id", Var::One(ID)), ("location", Var::One(PLACE))],
            ),
            (
                market(
                    &[PLACE, "Lyon"],
                    Some("2026-10-01"),
                    Some("2026-10-05"),
                    Some("Hotel room"),
                ),
                "Market Comparison",
                with_dates(vec![
                    ("locations", Var::List(&[PLACE, "Lyon"])),
                    ("property_type", Var::One("Hotel room")),
                ]),
            ),
            (portfolio(ID), "Host Portfolio", vec![("id", Var::One(ID))]),
            (
                sentiment(ID, 10),
                "Review Sentiment",
                vec![("id", Var::One(ID)), ("max_pages", Var::One("10"))],
            ),
            (
                positioning(ID, Some(PLACE)),
                "Competitive Positioning",
                vec![("id", Var::One(ID)), ("location", Var::One(PLACE))],
            ),
            (
                positioning(ID, None),
                "Competitive Positioning",
                vec![("id", Var::One(ID))],
            ),
            (
                pricing(ID, None, 6),
                "Optimal Pricing",
                vec![("id", Var::One(ID)), ("months", Var::One("6"))],
            ),
        ];
        let mut covered = std::collections::HashSet::new();
        for (built, name, vars) in &cases {
            assert_eq!(built, &expand(template(name), vars), "{name}");
            covered.insert(*name);
        }
        let all: std::collections::HashSet<_> = TEMPLATES.iter().map(|t| t.name).collect();
        assert_eq!(covered, all, "every template has a case");
    }

    #[test]
    fn reserved_and_non_ascii_characters_are_encoded() {
        let paris = SearchParams {
            location: "Paris, France".into(),
            ..SearchParams::default()
        };
        assert_eq!(search(&paris), "airbnb://search/Paris%2C%20France");
        let sao = SearchParams {
            location: "São Paulo/Centro".into(),
            ..SearchParams::default()
        };
        assert_eq!(search(&sao), "airbnb://search/S%C3%A3o%20Paulo%2FCentro");
        assert_eq!(
            market(&["Paris, France", "Lyon"], None, None, None),
            "airbnb://analysis/market/Paris%2C%20France,Lyon"
        );
    }

    #[test]
    fn inputs_that_change_the_result_change_the_uri() {
        assert_ne!(reviews("42", None), reviews("42", Some("24")));
        assert_ne!(calendar("42", 3), calendar("42", 12));
        let page1 = SearchParams {
            location: "Paris".into(),
            ..SearchParams::default()
        };
        let page2 = SearchParams {
            location: "Paris".into(),
            cursor: Some("p2".into()),
            ..SearchParams::default()
        };
        assert_ne!(search(&page1), search(&page2));
        // MCP-3: a listing id and a location spelled the same used to share a key.
        assert_ne!(
            revenue(Some("12345"), None, 12),
            revenue(None, Some("12345"), 12)
        );
        // MCP-3: ids ["1","2"] and location "1_2" used to share a key.
        assert_ne!(
            compare_ids(&["1", "2"]),
            compare_location("1_2", 20, None, None, None)
        );
        assert_ne!(
            market(&["a,b", "c"], None, None, None),
            market(&["a", "b,c"], None, None, None)
        );
    }

    #[test]
    fn canonicalize_is_idempotent_on_builder_output() {
        let params = SearchParams {
            location: "São Paulo, Brasil".into(),
            cursor: Some("x y".into()),
            ..SearchParams::default()
        };
        for uri in [
            search(&params),
            market(
                &["Paris, France", "Lyon"],
                Some("2026-10-01"),
                Some("2026-10-05"),
                None,
            ),
            reviews("42", Some("24")),
            revenue(None, Some("Rome, Italy"), 12),
            compare_ids(&["1", "2"]),
        ] {
            assert_eq!(canonicalize(&uri), uri);
        }
    }

    #[test]
    fn canonicalize_normalises_equivalent_spellings() {
        assert_eq!(
            canonicalize("airbnb://search/Paris%2c%20France"),
            "airbnb://search/Paris%2C%20France"
        );
        assert_eq!(
            canonicalize("airbnb://search/Paris%2C France"),
            "airbnb://search/Paris%2C%20France"
        );
        assert_eq!(
            canonicalize("airbnb://search/São Paulo"),
            "airbnb://search/S%C3%A3o%20Paulo"
        );
        assert_eq!(canonicalize("airbnb://search/a%7Eb"), "airbnb://search/a~b");
        assert_eq!(
            canonicalize("https://example.com/x y"),
            "https://example.com/x y"
        );
    }

    #[test]
    fn eighteen_distinct_templates_one_per_tool() {
        assert_eq!(TEMPLATES.len(), 18);
        let mut seen = std::collections::HashSet::new();
        // Names must be unique too: the tests look templates up by name.
        let mut names = std::collections::HashSet::new();
        for t in &TEMPLATES {
            assert!(t.uri_template.starts_with(SCHEME), "{}", t.uri_template);
            assert!(seen.insert(t.uri_template), "duplicate {}", t.uri_template);
            assert!(names.insert(t.name), "duplicate name {}", t.name);
        }
        // MCP-12: the old catch-all template said "trends"; the real segment is "price-trends".
        assert!(
            TEMPLATES
                .iter()
                .any(|t| t.uri_template.contains("/price-trends/"))
        );
        assert!(!TEMPLATES.iter().any(|t| t.uri_template.contains("{type}")));
    }
}
