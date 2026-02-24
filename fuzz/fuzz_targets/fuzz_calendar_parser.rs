#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(html) = std::str::from_utf8(data) {
        let _ = mcp_airbnb::adapters::scraper::calendar_parser::parse_price_calendar(
            html,
            "12345",
        );
    }
});
