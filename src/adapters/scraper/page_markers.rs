//! Recognise a genuine Airbnb listing page, so the HTML fallbacks can tell
//! "listing without data of this kind" apart from "not a listing page at all"
//! (bot challenge, consent wall, error page served with HTTP 200).

use scraper::{Html, Selector};

/// True when the page identifies itself as the page of `listing_id` through
/// `<link rel="canonical">`, `<meta property="og:url">`, or an embedded
/// `"canonicalUrl":"…/rooms/{id}"` JSON value (the PDP SEO data).
pub(crate) fn is_listing_page(document: &Html, html: &str, listing_id: &str) -> bool {
    let needle = format!("/rooms/{listing_id}");
    let tagged = Selector::parse(r#"link[rel="canonical"], meta[property="og:url"]"#).is_ok_and(
        |selector| {
            document.select(&selector).any(|element| {
                element
                    .value()
                    .attr("href")
                    .or_else(|| element.value().attr("content"))
                    .is_some_and(|url| points_to_listing(url, &needle))
            })
        },
    );
    tagged
        || html
            .split("\"canonicalUrl\":\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .any(|url| points_to_listing(url, &needle))
}

/// `url` contains `needle` (`/rooms/{id}`) followed by the end of the path, so
/// that `/rooms/1234` does not match listing `123`.
fn points_to_listing(url: &str, needle: &str) -> bool {
    url.match_indices(needle).any(|(start, matched)| {
        let rest = url.get(start + matched.len()..).unwrap_or_default();
        rest.chars()
            .next()
            .is_none_or(|c| matches!(c, '?' | '/' | '#'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Html {
        Html::parse_document(html)
    }

    #[test]
    fn canonical_link_marks_the_listing_page() {
        let html = r#"<html><head><link rel="canonical" href="https://www.airbnb.com/rooms/123"></head></html>"#;
        assert!(is_listing_page(&doc(html), html, "123"));
    }

    #[test]
    fn og_url_with_query_marks_the_listing_page() {
        let html = r#"<html><head><meta property="og:url" content="https://www.airbnb.com/rooms/123?source_impression_id=x"></head></html>"#;
        assert!(is_listing_page(&doc(html), html, "123"));
    }

    #[test]
    fn embedded_canonical_url_marks_the_listing_page() {
        let html = r#"<html><body><script type="application/json">{"canonicalUrl":"https://www.airbnb.com/rooms/123","x":1}</script></body></html>"#;
        assert!(is_listing_page(&doc(html), html, "123"));
    }

    #[test]
    fn another_listing_id_is_not_a_match() {
        let html = r#"<html><head><link rel="canonical" href="https://www.airbnb.com/rooms/1234"></head></html>"#;
        assert!(!is_listing_page(&doc(html), html, "123"));
    }

    #[test]
    fn bot_challenge_page_is_not_a_listing_page() {
        let html = "<html><body><h1>Please verify you are a human</h1></body></html>";
        assert!(!is_listing_page(&doc(html), html, "123"));
    }
}
