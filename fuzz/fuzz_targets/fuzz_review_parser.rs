#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(html) = std::str::from_utf8(data) {
        let _ = mcp_airbnb::adapters::scraper::review_parser::parse_reviews(
            html,
            "12345",
        );
    }
});
