//! Currency and locale pinning shared by every outbound Airbnb request.
//!
//! Airbnb picks the currency and language from the IP address and cookies
//! unless the request states them. Every request path (GraphQL GET, GraphQL
//! POST, HTML pages) pins both, so prices from different sources share one
//! currency.

use url::Url;

/// `Accept-Language` header value for a pinned locale.
pub fn accept_language(locale: &str) -> String {
    let locale = locale.trim();
    if locale.is_empty() || locale.eq_ignore_ascii_case("en") {
        "en-US,en;q=0.9".to_string()
    } else {
        format!("{locale},en;q=0.8")
    }
}

/// Append `currency` and `locale` query parameters unless `url` already has them.
pub fn pin_currency_and_locale(url: &mut Url, currency: &str, locale: &str) {
    let add_currency = !url.query_pairs().any(|(key, _)| key == "currency");
    let add_locale = !url.query_pairs().any(|(key, _)| key == "locale");
    if add_currency || add_locale {
        let mut pairs = url.query_pairs_mut();
        if add_currency {
            pairs.append_pair("currency", currency);
        }
        if add_locale {
            pairs.append_pair("locale", locale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_prefers_the_pinned_locale() {
        assert_eq!(accept_language("en"), "en-US,en;q=0.9");
        assert_eq!(accept_language("fr"), "fr,en;q=0.8");
        assert_eq!(accept_language(""), "en-US,en;q=0.9");
    }

    #[test]
    fn pinning_adds_only_missing_parameters() {
        let mut url = Url::parse("https://www.airbnb.com/rooms/1?currency=GBP").unwrap();
        pin_currency_and_locale(&mut url, "EUR", "fr");
        assert_eq!(
            url.as_str(),
            "https://www.airbnb.com/rooms/1?currency=GBP&locale=fr"
        );
        let mut bare = Url::parse("https://www.airbnb.com/rooms/1").unwrap();
        pin_currency_and_locale(&mut bare, "EUR", "fr");
        assert_eq!(
            bare.as_str(),
            "https://www.airbnb.com/rooms/1?currency=EUR&locale=fr"
        );
    }
}
