#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
            let _ = mcp_airbnb::adapters::graphql::parsers::detail::parse_detail_response(
                &json,
                "12345",
                "https://www.airbnb.com",
            );
        }
    }
});
