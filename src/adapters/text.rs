//! Text helpers shared by the Airbnb parsers: HTML descriptions, rating
//! labels, host labels and ids, room counts and "Type in Place" titles.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use scraper::Html;
use serde_json::Value;

/// Plain text of an Airbnb HTML snippet (descriptions, highlights).
///
/// `<br>`, `</p>` and `</li>` become line breaks, tags are removed and the
/// HTML parser decodes entities exactly once (`&amp;lt;` stays `&lt;`, an
/// escaped `&lt; 100` stays text). Runs of blank lines collapse to one, and
/// no-break spaces become spaces.
pub fn html_to_text(html: &str) -> String {
    let mut prepared = html.to_string();
    for tag in [
        "<br />", "<br/>", "<br>", "<BR />", "<BR/>", "<BR>", "</p>", "</P>", "</li>", "</LI>",
    ] {
        prepared = prepared.replace(tag, "\n");
    }
    let fragment = Html::parse_fragment(&prepared);
    let raw = fragment
        .root_element()
        .text()
        .collect::<String>()
        .replace('\u{a0}', " ");
    let mut out = String::with_capacity(raw.len());
    let mut blank_lines = 0;
    for line in raw.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank_lines += 1;
            if blank_lines > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank_lines = 0;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.trim_end().to_string()
}

/// Rating and review count from `avgRatingLocalized` (`"4.95 (1,234)"`,
/// `"4,93 (1 017)"`, `"New"`).
pub fn parse_rating_and_count(label: &str) -> (Option<f64>, Option<u32>) {
    let (head, tail) = label.split_once('(').unwrap_or((label, ""));
    let rating = head.split_whitespace().next().and_then(parse_rating);
    let count = tail.split(')').next().and_then(digits_u32);
    (rating, count)
}

/// Rating and review count from `avgRatingA11yLabel`
/// (`"4.95 out of 5 average rating,  74 reviews"`).
pub fn parse_rating_a11y_label(label: &str) -> (Option<f64>, Option<u32>) {
    let words: Vec<&str> = label.split_whitespace().collect();
    let rating = words.first().and_then(|word| parse_rating(word));
    let count = words.windows(2).find_map(|pair| {
        pair[1]
            .to_lowercase()
            .starts_with("review")
            .then(|| digits_u32(pair[0]))
            .flatten()
    });
    (rating, count)
}

/// A star rating in `0.0..=5.0`, accepting a decimal comma.
fn parse_rating(token: &str) -> Option<f64> {
    token
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|rating| rating.is_finite() && (0.0..=5.0).contains(rating))
}

/// All ASCII digits of `s` as one number (`"1,234"` → 1234, `"1 017"` → 1017).
fn digits_u32(s: &str) -> Option<u32> {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `s` without `prefix`, compared ASCII-case-insensitively on the original
/// bytes (so a non-ASCII character can never shift the slice).
fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Languages of a host highlight such as `"Speaks English and French"` or
/// `"Languages: English"`.
pub fn languages_from_highlight(title: &str) -> Option<Vec<String>> {
    let rest = strip_prefix_ignore_ascii_case(title, "speaks ")
        .or_else(|| strip_prefix_ignore_ascii_case(title, "languages:"))
        .or_else(|| strip_prefix_ignore_ascii_case(title, "language:"))?;
    let languages: Vec<String> = rest
        .split([',', '&'])
        .flat_map(|part| part.split(" and "))
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(str::to_string)
        .collect();
    (!languages.is_empty()).then_some(languages)
}

/// `"Response rate: 100%"` → `"100%"` (Airbnb's string already carries the
/// label that the displays print).
pub fn normalize_response_rate(s: &str) -> String {
    let trimmed = s.trim();
    strip_prefix_ignore_ascii_case(trimmed, "response rate:")
        .map_or(trimmed, str::trim)
        .to_string()
}

/// `"Responds within an hour"` → `"within an hour"`.
pub fn normalize_response_time(s: &str) -> String {
    let trimmed = s.trim();
    strip_prefix_ignore_ascii_case(trimmed, "response time:")
        .or_else(|| strip_prefix_ignore_ascii_case(trimmed, "responds "))
        .map_or(trimmed, str::trim)
        .to_string()
}

/// Numeric user id from an Airbnb user id: digits are kept, base64 relay ids
/// `DemandUser:<n>` / `User:<n>` are decoded to `<n>`, anything else is
/// returned trimmed.
pub fn normalize_user_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return trimmed.to_string();
    }
    if let Ok(bytes) = STANDARD.decode(trimmed)
        && let Ok(decoded) = String::from_utf8(bytes)
        && let Some((kind, id)) = decoded.split_once(':')
        && matches!(kind, "DemandUser" | "User")
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_digit())
    {
        return id.to_string();
    }
    trimmed.to_string()
}

/// User id from JSON (string or number), normalized with [`normalize_user_id`].
pub fn user_id_from_json(value: &Value) -> Option<String> {
    let raw = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|n| n.to_string()))?;
    let id = normalize_user_id(&raw);
    (!id.is_empty()).then_some(id)
}

/// Host name from a search card `HOSTINFO` line. Only `"Hosted by <name>"`
/// names a host; `"Business host"` and `"Individual host"` are host types.
pub fn host_name_from_hostinfo(body: &str) -> Option<String> {
    let name = strip_prefix_ignore_ascii_case(body.trim(), "hosted by ")?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Split an Airbnb "Type in Place" line (`"Entire rental unit in Lyon, France"`,
/// `"Room in hotel in Lyon"`) into `(type, place)` at the last `" in "`.
pub fn split_type_and_place(line: &str) -> Option<(String, String)> {
    let (kind, place) = line.rsplit_once(" in ")?;
    let (kind, place) = (kind.trim(), place.trim());
    (!kind.is_empty() && !place.is_empty()).then(|| (kind.to_string(), place.to_string()))
}

/// Capacity counts read from overview labels (`"4 guests"`, `"1.5 baths"`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoomCounts {
    /// Maximum number of guests.
    pub guests: Option<u32>,
    /// Bedrooms (`Some(0)` for a studio).
    pub bedrooms: Option<u32>,
    /// Beds.
    pub beds: Option<u32>,
    /// Bathrooms, with halves (`0.5` for a half-bath).
    pub bathrooms: Option<f64>,
}

impl RoomCounts {
    /// Keep the counts known in `self` and take the unknown ones from `other`.
    #[must_use]
    pub fn or_fill(self, other: Self) -> Self {
        Self {
            guests: self.guests.or(other.guests),
            bedrooms: self.bedrooms.or(other.bedrooms),
            beds: self.beds.or(other.beds),
            bathrooms: self.bathrooms.or(other.bathrooms),
        }
    }
}

/// Parse capacity labels. The first label of each kind wins, and a label
/// without a readable number never erases a known count. Keywords are
/// English (requests pin `locale=en` by default).
pub fn parse_room_counts<I, S>(items: I) -> RoomCounts
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut counts = RoomCounts::default();
    for item in items {
        let lower = item.as_ref().to_lowercase();
        if lower.contains("studio") {
            counts.bedrooms = counts.bedrooms.or(Some(0));
        } else if lower.contains("bedroom") {
            counts.bedrooms = counts.bedrooms.or_else(|| leading_u32(&lower));
        } else if lower.contains("bath") {
            let half = lower.contains("half").then_some(0.5);
            counts.bathrooms = counts.bathrooms.or_else(|| leading_f64(&lower).or(half));
        } else if lower.contains("bed") {
            counts.beds = counts.beds.or_else(|| leading_u32(&lower));
        } else if lower.contains("guest") {
            counts.guests = counts.guests.or_else(|| leading_u32(&lower));
        }
    }
    counts
}

fn leading_u32(s: &str) -> Option<u32> {
    s.split_whitespace()
        .find_map(|word| word.trim_end_matches('+').parse::<u32>().ok())
}

fn leading_f64(s: &str) -> Option<f64> {
    s.split_whitespace()
        .find_map(|word| {
            word.trim_end_matches('+')
                .replace(',', ".")
                .parse::<f64>()
                .ok()
        })
        .filter(|n| n.is_finite() && *n >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_keeps_escaped_brackets_and_line_breaks() {
        assert_eq!(
            html_to_text("Price &lt; 100 &amp; quiet &gt; noisy"),
            "Price < 100 & quiet > noisy"
        );
        assert_eq!(html_to_text("Kids &amp;lt;12 free"), "Kids &lt;12 free");
        assert_eq!(
            html_to_text("Near metro<br />Walk to Louvre &amp; Seine"),
            "Near metro\nWalk to Louvre & Seine"
        );
        assert_eq!(html_to_text("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(html_to_text("A<br /><br /><br /><br />B"), "A\n\nB");
        assert_eq!(html_to_text("100&nbsp;m"), "100 m");
    }

    #[test]
    fn rating_labels_accept_thousands_separators_and_comma_decimals() {
        assert_eq!(
            parse_rating_and_count("4.95 (1,234)"),
            (Some(4.95), Some(1234))
        );
        assert_eq!(
            parse_rating_and_count("4,93 (1 017)"),
            (Some(4.93), Some(1017))
        );
        assert_eq!(
            parse_rating_and_count("4.98 (126)"),
            (Some(4.98), Some(126))
        );
        assert_eq!(parse_rating_and_count("New"), (None, None));
        assert_eq!(parse_rating_and_count(""), (None, None));
        assert_eq!(
            parse_rating_a11y_label("4.95 out of 5 average rating,  74 reviews"),
            (Some(4.95), Some(74))
        );
        assert_eq!(
            parse_rating_a11y_label("4.9 out of 5 average rating, 1,033 reviews"),
            (Some(4.9), Some(1033))
        );
    }

    #[test]
    fn speaks_prefix_is_matched_on_the_original_bytes() {
        assert_eq!(
            languages_from_highlight("Speaks English and French"),
            Some(vec!["English".to_string(), "French".to_string()])
        );
        assert_eq!(
            languages_from_highlight("Speaks English, French, and Spanish"),
            Some(vec![
                "English".to_string(),
                "French".to_string(),
                "Spanish".to_string()
            ])
        );
        assert_eq!(
            languages_from_highlight("Languages: English"),
            Some(vec!["English".to_string()])
        );
        // U+212A KELVIN SIGN lower-cases to 'k'; the old code matched the
        // lower-cased prefix and then sliced the original string at byte 7.
        assert_eq!(
            languages_from_highlight("Spea\u{212a}s English and French"),
            None
        );
        assert_eq!(languages_from_highlight("Lives in Paris"), None);
    }

    #[test]
    fn host_labels_are_normalized() {
        assert_eq!(normalize_response_rate("Response rate: 100%"), "100%");
        assert_eq!(normalize_response_rate("98%"), "98%");
        assert_eq!(
            normalize_response_time("Responds within an hour"),
            "within an hour"
        );
        assert_eq!(normalize_response_time("within a day"), "within a day");
        assert_eq!(
            host_name_from_hostinfo("Hosted by Marie"),
            Some("Marie".to_string())
        );
        assert_eq!(host_name_from_hostinfo("Business host"), None);
        assert_eq!(host_name_from_hostinfo("Individual host"), None);
    }

    #[test]
    fn relay_user_ids_are_decoded_to_digits() {
        assert_eq!(normalize_user_id("RGVtYW5kVXNlcjoxMDAwMDAx"), "1000001");
        assert_eq!(normalize_user_id("VXNlcjoxMDAwMDAy"), "1000002");
        assert_eq!(normalize_user_id("12345"), "12345");
        // A relay id of another kind is kept as-is.
        assert_eq!(
            normalize_user_id("U3RheUxpc3Rpbmc6Mzg4MTc5Njk="),
            "U3RheUxpc3Rpbmc6Mzg4MTc5Njk="
        );
        assert_eq!(
            user_id_from_json(&serde_json::json!(12345)),
            Some("12345".to_string())
        );
        assert_eq!(user_id_from_json(&serde_json::json!("")), None);
    }

    #[test]
    fn room_counts_handle_half_baths_studios_and_plus_signs() {
        let counts = parse_room_counts(["4 guests", "2 bedrooms", "3 beds", "1.5 baths"]);
        assert_eq!(
            counts,
            RoomCounts {
                guests: Some(4),
                bedrooms: Some(2),
                beds: Some(3),
                bathrooms: Some(1.5)
            }
        );
        assert_eq!(parse_room_counts(["Studio"]).bedrooms, Some(0));
        assert_eq!(parse_room_counts(["Half-bath"]).bathrooms, Some(0.5));
        assert_eq!(parse_room_counts(["16+ guests"]).guests, Some(16));
        assert_eq!(parse_room_counts(["1 private bath"]).bathrooms, Some(1.0));
        let title = "Rental unit \u{b7} \u{2605}4.9 \u{b7} 1 bedroom \u{b7} 1 bed \u{b7} 2.5 baths";
        let parts: Vec<&str> = title.split('\u{b7}').map(str::trim).collect();
        assert_eq!(parse_room_counts(parts).bathrooms, Some(2.5));
        let first_wins = parse_room_counts(["4 guests", "16+ guests"]);
        assert_eq!(first_wins.guests, Some(4));
        let filled = RoomCounts {
            guests: Some(2),
            ..RoomCounts::default()
        }
        .or_fill(RoomCounts {
            guests: Some(9),
            beds: Some(1),
            ..RoomCounts::default()
        });
        assert_eq!((filled.guests, filled.beds), (Some(2), Some(1)));
    }

    #[test]
    fn type_and_place_split_on_the_last_in() {
        assert_eq!(
            split_type_and_place("Hotel in V\u{e9}nissieux, France"),
            Some(("Hotel".to_string(), "V\u{e9}nissieux, France".to_string()))
        );
        assert_eq!(
            split_type_and_place("Room in hotel in Lyon"),
            Some(("Room in hotel".to_string(), "Lyon".to_string()))
        );
        assert_eq!(split_type_and_place("Example Hotel Lyon"), None);
    }
}
