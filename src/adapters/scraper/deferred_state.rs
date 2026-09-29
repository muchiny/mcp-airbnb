//! Access to the JSON Airbnb embeds in listing and search pages.
//!
//! Pages carry their data in `<script id="data-deferred-state-0">` blocks.
//! Each block wraps query results in a `niobeClientData` or
//! `niobeMinimalClientData` array of `[query_key, payload]` pairs; older pages
//! use a `__NEXT_DATA__` script instead. Parsers parse the page into a DOM
//! once and read every tier from that document.

use scraper::{Html, Selector};
use serde_json::Value;

/// Parse the `__NEXT_DATA__` script of `document`, if present.
pub fn next_data_json(document: &Html) -> Option<Value> {
    let selector = Selector::parse("script#__NEXT_DATA__").ok()?;
    let script = document.select(&selector).next()?;
    serde_json::from_str(&script.text().collect::<String>()).ok()
}

/// Parse every `data-deferred-state` script of `document` as JSON.
pub fn deferred_state_json(document: &Html) -> Vec<Value> {
    let Ok(selector) =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']")
    else {
        return Vec::new();
    };
    document
        .select(&selector)
        .filter_map(|script| serde_json::from_str(&script.text().collect::<String>()).ok())
        .collect()
}

/// Query payloads wrapped in `niobeClientData` / `niobeMinimalClientData`
/// (`[query_key, payload]` pairs), in document order.
pub fn niobe_payloads(state: &Value) -> Vec<&Value> {
    ["niobeClientData", "niobeMinimalClientData"]
        .iter()
        .filter_map(|key| state.get(*key).and_then(Value::as_array))
        .flatten()
        .filter_map(|entry| entry.as_array().and_then(|pair| pair.get(1)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_niobe_wrappers_are_unwrapped_in_document_order() {
        let a = serde_json::json!({"a": 1});
        let b = serde_json::json!({"b": 2});
        for key in ["niobeClientData", "niobeMinimalClientData"] {
            let html = crate::test_helpers::niobe_page(key, &[("Q1:{}", &a), ("Q2:{}", &b)]);
            let document = Html::parse_document(&html);
            let states = deferred_state_json(&document);
            assert_eq!(states.len(), 1);
            assert_eq!(niobe_payloads(&states[0]), vec![&a, &b]);
        }
    }

    #[test]
    fn next_data_is_read_when_present() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">{"props":{"x":1}}</script></head></html>"#;
        let document = Html::parse_document(html);
        assert_eq!(
            next_data_json(&document),
            Some(serde_json::json!({"props": {"x": 1}}))
        );
        assert!(deferred_state_json(&document).is_empty());
    }
}
