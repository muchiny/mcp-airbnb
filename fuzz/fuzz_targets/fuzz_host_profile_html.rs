#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = mcp_airbnb::fuzz_support::scraper_host_profile_html(data);
});
