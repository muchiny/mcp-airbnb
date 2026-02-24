#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
            let _ = mcp_airbnb::adapters::graphql::parsers::search::parse_search_response(
                &json,
                "https://www.airbnb.com",
            );
        }
    }
});
