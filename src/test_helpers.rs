use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::{CalendarDay, PriceCalendar};
use crate::domain::listing::{Listing, ListingDetail, SearchResult};
use crate::domain::review::{Review, ReviewsPage, ReviewsSummary};
use crate::domain::search_params::SearchParams;
use crate::error::Result;
use crate::ports::airbnb_client::AirbnbClient;

type SearchFn = Box<dyn Fn(&SearchParams) -> Result<SearchResult> + Send + Sync>;
type DetailFn = Box<dyn Fn(&str) -> Result<ListingDetail> + Send + Sync>;
type ReviewsFn = Box<dyn Fn(&str, Option<&str>) -> Result<ReviewsPage> + Send + Sync>;
type CalendarFn = Box<dyn Fn(&str, u32) -> Result<PriceCalendar> + Send + Sync>;
type HostProfileFn = Box<dyn Fn(&str) -> Result<HostProfile> + Send + Sync>;
type NeighborhoodFn = Box<dyn Fn(&SearchParams) -> Result<NeighborhoodStats> + Send + Sync>;
type OccupancyFn = Box<dyn Fn(&str, u32) -> Result<OccupancyEstimate> + Send + Sync>;

#[allow(clippy::struct_field_names)]
pub struct MockAirbnbClient {
    search_fn: Mutex<SearchFn>,
    detail_fn: Mutex<DetailFn>,
    reviews_fn: Mutex<ReviewsFn>,
    calendar_fn: Mutex<CalendarFn>,
    host_profile_fn: Mutex<HostProfileFn>,
    neighborhood_fn: Mutex<NeighborhoodFn>,
    occupancy_fn: Mutex<OccupancyFn>,
}

impl Default for MockAirbnbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockAirbnbClient {
    pub fn new() -> Self {
        Self {
            search_fn: Mutex::new(Box::new(|_| Ok(make_search_result(vec![])))),
            detail_fn: Mutex::new(Box::new(|id| Ok(make_listing_detail(id)))),
            reviews_fn: Mutex::new(Box::new(|id, _| Ok(make_reviews_page(id, vec![])))),
            calendar_fn: Mutex::new(Box::new(|id, _| Ok(make_price_calendar(id, vec![])))),
            host_profile_fn: Mutex::new(Box::new(|_| Ok(make_host_profile("Test Host")))),
            neighborhood_fn: Mutex::new(Box::new(|params| {
                Ok(make_neighborhood_stats(&params.location))
            })),
            occupancy_fn: Mutex::new(Box::new(|id, _| Ok(make_occupancy_estimate(id)))),
        }
    }

    #[must_use]
    pub fn with_search(
        self,
        f: impl Fn(&SearchParams) -> Result<SearchResult> + Send + Sync + 'static,
    ) -> Self {
        *self.search_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_detail(
        self,
        f: impl Fn(&str) -> Result<ListingDetail> + Send + Sync + 'static,
    ) -> Self {
        *self.detail_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_reviews(
        self,
        f: impl Fn(&str, Option<&str>) -> Result<ReviewsPage> + Send + Sync + 'static,
    ) -> Self {
        *self.reviews_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_calendar(
        self,
        f: impl Fn(&str, u32) -> Result<PriceCalendar> + Send + Sync + 'static,
    ) -> Self {
        *self.calendar_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_host_profile(
        self,
        f: impl Fn(&str) -> Result<HostProfile> + Send + Sync + 'static,
    ) -> Self {
        *self.host_profile_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_neighborhood(
        self,
        f: impl Fn(&SearchParams) -> Result<NeighborhoodStats> + Send + Sync + 'static,
    ) -> Self {
        *self.neighborhood_fn.lock().unwrap() = Box::new(f);
        self
    }

    #[must_use]
    pub fn with_occupancy(
        self,
        f: impl Fn(&str, u32) -> Result<OccupancyEstimate> + Send + Sync + 'static,
    ) -> Self {
        *self.occupancy_fn.lock().unwrap() = Box::new(f);
        self
    }
}

#[async_trait]
impl AirbnbClient for MockAirbnbClient {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        let f = self.search_fn.lock().unwrap();
        f(params)
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        let f = self.detail_fn.lock().unwrap();
        f(id)
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        let f = self.reviews_fn.lock().unwrap();
        f(id, cursor)
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        let f = self.calendar_fn.lock().unwrap();
        f(id, months)
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        let f = self.host_profile_fn.lock().unwrap();
        f(listing_id)
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        let f = self.neighborhood_fn.lock().unwrap();
        f(params)
    }

    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate> {
        let f = self.occupancy_fn.lock().unwrap();
        f(id, months)
    }
}

// --- Factory functions ---

pub fn make_listing(id: &str, name: &str, price: f64) -> Listing {
    Listing {
        id: id.to_string(),
        name: name.to_string(),
        location: "Test City".to_string(),
        price_per_night: price,
        currency: "$".to_string(),
        rating: Some(4.5),
        review_count: 10,
        thumbnail_url: None,
        property_type: Some("Apartment".to_string()),
        host_name: Some("Test Host".to_string()),
        host_id: None,
        url: format!("https://www.airbnb.com/rooms/{id}"),
        is_superhost: None,
        is_guest_favorite: None,
        instant_book: None,
        total_price: None,
        photos: vec![],
        latitude: None,
        longitude: None,
    }
}

pub fn make_listing_detail(id: &str) -> ListingDetail {
    ListingDetail {
        id: id.to_string(),
        name: "Test Listing".to_string(),
        location: "Test City".to_string(),
        description: "A wonderful test place".to_string(),
        price_per_night: 100.0,
        currency: "$".to_string(),
        rating: Some(4.8),
        review_count: 25,
        property_type: Some("Apartment".to_string()),
        host_name: Some("Test Host".to_string()),
        url: format!("https://www.airbnb.com/rooms/{id}"),
        amenities: vec!["WiFi".to_string(), "Kitchen".to_string()],
        house_rules: vec!["No smoking".to_string()],
        latitude: Some(48.8566),
        longitude: Some(2.3522),
        photos: vec!["https://example.com/photo1.jpg".to_string()],
        bedrooms: Some(2),
        beds: Some(3),
        bathrooms: Some(1.5),
        max_guests: Some(4),
        check_in_time: Some("15:00".to_string()),
        check_out_time: Some("11:00".to_string()),
        host_id: None,
        host_is_superhost: None,
        host_response_rate: None,
        host_response_time: None,
        host_joined: None,
        host_total_listings: None,
        host_languages: vec![],
        cancellation_policy: None,
        instant_book: None,
        cleaning_fee: None,
        service_fee: None,
        neighborhood: None,
    }
}

pub fn make_review(author: &str, comment: &str) -> Review {
    Review {
        author: author.to_string(),
        date: "2025-01-15".to_string(),
        rating: Some(4.0),
        comment: comment.to_string(),
        response: None,
        reviewer_location: None,
        language: None,
        is_translated: None,
    }
}

pub fn make_reviews_page(listing_id: &str, reviews: Vec<Review>) -> ReviewsPage {
    ReviewsPage {
        listing_id: listing_id.to_string(),
        summary: None,
        reviews,
        next_cursor: None,
    }
}

pub fn make_reviews_summary() -> ReviewsSummary {
    ReviewsSummary {
        overall_rating: 4.7,
        total_reviews: 50,
        cleanliness: Some(4.8),
        accuracy: Some(4.9),
        communication: Some(4.7),
        location: Some(4.6),
        check_in: Some(4.9),
        value: Some(4.5),
    }
}

pub fn make_price_calendar(listing_id: &str, days: Vec<CalendarDay>) -> PriceCalendar {
    PriceCalendar {
        listing_id: listing_id.to_string(),
        currency: "$".to_string(),
        days,
        average_price: None,
        occupancy_rate: None,
        min_price: None,
        max_price: None,
    }
}

pub fn make_calendar_day(date: &str, price: Option<f64>, available: bool) -> CalendarDay {
    CalendarDay {
        date: date.to_string(),
        price,
        available,
        min_nights: Some(2),
        max_nights: None,
        closed_to_arrival: None,
        closed_to_departure: None,
        unavailability_reason: None,
    }
}

pub fn make_search_result(listings: Vec<Listing>) -> SearchResult {
    SearchResult {
        listings,
        total_count: None,
        next_cursor: None,
    }
}

pub fn make_host_profile(name: &str) -> HostProfile {
    HostProfile {
        host_id: Some("12345".to_string()),
        name: name.to_string(),
        is_superhost: Some(true),
        response_rate: Some("98%".to_string()),
        response_time: Some("within an hour".to_string()),
        member_since: Some("2018".to_string()),
        languages: vec!["English".to_string()],
        total_listings: Some(3),
        description: None,
        profile_picture_url: None,
        identity_verified: Some(true),
    }
}

pub fn make_neighborhood_stats(location: &str) -> NeighborhoodStats {
    NeighborhoodStats {
        location: location.to_string(),
        total_listings: 0,
        average_price: None,
        median_price: None,
        price_range: None,
        average_rating: None,
        property_type_distribution: vec![],
        superhost_percentage: None,
        currency: None,
        priced_listings: 0,
    }
}

pub fn make_occupancy_estimate(listing_id: &str) -> OccupancyEstimate {
    OccupancyEstimate {
        listing_id: listing_id.to_string(),
        period_start: String::new(),
        period_end: String::new(),
        total_days: 0,
        occupied_days: 0,
        available_days: 0,
        occupancy_rate: 0.0,
        past_days_excluded: 0,
        blocked_days_excluded: 0,
        currency: "$".into(),
        average_available_price: None,
        weekend_avg_price: None,
        weekday_avg_price: None,
        monthly_breakdown: vec![],
    }
}

/// Root of the anonymized Airbnb captures committed for tests
/// (see `tests/fixtures/airbnb/2026-09/README.md`).
pub const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/airbnb/2026-09/"
);

/// Load a committed, anonymized Airbnb capture as JSON.
///
/// `rel` is relative to `FIXTURE_DIR`, e.g. `"graphql/StaysSearch.response.json"`.
/// Panics with the offending path when the file is missing or is not JSON
/// (this module only exists in test builds).
pub fn fixture_json(rel: &str) -> serde_json::Value {
    let path = format!("{FIXTURE_DIR}{rel}");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("fixture {path} is not valid JSON: {e}"))
}

/// Wrap query payloads the way Airbnb embeds them in a page:
/// `<script id="data-deferred-state-0">{"<wrapper_key>": [[key, payload], ...]}</script>`.
/// `wrapper_key` is `niobeClientData` or `niobeMinimalClientData`.
pub fn niobe_page(wrapper_key: &str, entries: &[(&str, &serde_json::Value)]) -> String {
    let pairs: Vec<serde_json::Value> = entries
        .iter()
        .map(|(key, payload)| {
            serde_json::Value::Array(vec![
                serde_json::Value::String((*key).to_string()),
                (*payload).clone(),
            ])
        })
        .collect();
    let mut state = serde_json::Map::new();
    state.insert(wrapper_key.to_string(), serde_json::Value::Array(pairs));
    format!(
        r#"<html><head><script id="data-deferred-state-0" type="application/json">{}</script></head><body></body></html>"#,
        serde_json::Value::Object(state)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED_FIXTURES: [&str; 8] = [
        "graphql/StaysSearch.request.json",
        "graphql/StaysSearch.response.json",
        "graphql/StaysSearch.validation_error.json",
        "graphql/StaysPdpReviewsQuery.response.json",
        "graphql/PdpAvailabilityCalendar.response.json",
        "graphql/StaysPdpSections.request.json",
        "graphql/StaysPdpSections.apartment.response.json",
        "graphql/StaysPdpSections.hotel.response.json",
    ];

    #[test]
    fn every_committed_fixture_loads_as_json() {
        for rel in COMMITTED_FIXTURES {
            assert!(fixture_json(rel).is_object(), "{rel} is not a JSON object");
        }
    }

    #[test]
    fn fixtures_are_anonymized() {
        for rel in COMMITTED_FIXTURES {
            let text = std::fs::read_to_string(format!("{FIXTURE_DIR}{rel}")).unwrap();
            assert!(
                !text.contains("a0.muscache.com"),
                "{rel} still contains Airbnb image URLs"
            );
            assert!(
                !text.contains("/users/show/"),
                "{rel} still contains profile links"
            );
            let real_uuids: Vec<&str> = uuids_in(&text)
                .into_iter()
                .filter(|uuid| *uuid != ZERO_UUID)
                .collect();
            assert!(
                real_uuids.is_empty(),
                "{rel} still contains {} non-zero UUID(s) (session, share or trace ids)",
                real_uuids.len()
            );
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            let mut host_ids = Vec::new();
            collect_string_values(&json, "hostId", &mut host_ids);
            for host_id in host_ids {
                assert!(
                    is_fake_user_id(&host_id),
                    "{rel} still contains a real numeric hostId"
                );
            }
        }
    }

    const ZERO_UUID: &str = "00000000-0000-0000-0000-000000000000";

    /// Every substring shaped like a UUID (8-4-4-4-12 hex digits).
    fn uuids_in(text: &str) -> Vec<&str> {
        const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
        let bytes = text.as_bytes();
        let is_uuid_at = |start: usize| {
            let mut i = start;
            for (n, len) in GROUPS.iter().enumerate() {
                if n > 0 {
                    if bytes.get(i) != Some(&b'-') {
                        return false;
                    }
                    i += 1;
                }
                for _ in 0..*len {
                    if !bytes.get(i).is_some_and(u8::is_ascii_hexdigit) {
                        return false;
                    }
                    i += 1;
                }
            }
            true
        };
        (0..bytes.len().saturating_sub(35))
            .filter(|&start| text.is_char_boundary(start) && is_uuid_at(start))
            .map(|start| &text[start..start + 36])
            .collect()
    }

    /// Collect every string value stored under `key`, at any depth.
    fn collect_string_values(value: &serde_json::Value, key: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == key
                        && let Some(s) = v.as_str()
                    {
                        out.push(s.to_string());
                    }
                    collect_string_values(v, key, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect_string_values(item, key, out);
                }
            }
            _ => {}
        }
    }

    /// `scripts/anonymize_fixtures.py` maps real user ids to 1000001, 1000002, ...
    fn is_fake_user_id(id: &str) -> bool {
        id.parse::<u64>()
            .is_ok_and(|n| (1_000_001..2_000_000).contains(&n))
    }

    #[test]
    fn uuids_in_finds_every_uuid_shape() {
        let text = "a=12345678-9abc-def0-1234-56789abcdef0&b=00000000-0000-0000-0000-000000000000 \
                    not-a-uuid 12345678-9abc-def0-1234";
        assert_eq!(
            uuids_in(text),
            vec!["12345678-9abc-def0-1234-56789abcdef0", ZERO_UUID]
        );
        assert!(uuids_in("é00000000-0000-0000-0000-00000000000").is_empty());
    }

    #[test]
    fn collect_string_values_walks_nested_objects_and_arrays() {
        let json = serde_json::json!({
            "hostId": "1",
            "a": [{"hostId": "2"}, {"b": {"hostId": "3", "other": "4"}}],
            "c": {"hostId": 5}
        });
        let mut found = Vec::new();
        collect_string_values(&json, "hostId", &mut found);
        found.sort();
        assert_eq!(found, vec!["1", "2", "3"]);
    }

    #[test]
    fn is_fake_user_id_accepts_only_the_anonymizer_range() {
        assert!(is_fake_user_id("1000001"));
        assert!(is_fake_user_id("1000074"));
        assert!(!is_fake_user_id("7654321"));
        assert!(!is_fake_user_id("1000000"));
        assert!(!is_fake_user_id("RGVtYW5kVXNlcjoxMDAwMDAy"));
    }

    #[test]
    #[should_panic(expected = "cannot read fixture")]
    fn missing_fixture_panics_with_its_path() {
        let _ = fixture_json("graphql/does-not-exist.json");
    }
}

#[cfg(test)]
mod p1b_fixture_tests {
    use super::{fixture_json, niobe_page};
    use serde_json::Value;

    #[test]
    fn p1b_pdp_fixtures_keep_the_real_shape() {
        for (file, pdp_type) in [
            ("p1b/pdp_hotel.json", "HOTEL"),
            ("p1b/pdp_apartment.json", "MARKETPLACE"),
        ] {
            let json = fixture_json(file);
            let sections = json
                .pointer("/data/presentation/stayProductDetailPage/sections/sections")
                .and_then(Value::as_array)
                .expect("sections array");
            assert!(sections.len() >= 10, "{file}: {} sections", sections.len());
            assert_eq!(
                json.pointer("/data/presentation/stayProductDetailPage/sections/metadata/pdpType")
                    .and_then(Value::as_str),
                Some(pdp_type)
            );
        }
        let apartment = fixture_json("p1b/pdp_apartment.json").to_string();
        assert!(apartment.contains("\"Host A\""));
        assert!(apartment.contains("Host bio redacted."));
    }

    #[test]
    fn p1b_search_and_calendar_fixtures_keep_the_real_shape() {
        let search = fixture_json("p1b/stays_search.json");
        let results = search
            .pointer("/data/presentation/staysSearch/results/searchResults")
            .and_then(Value::as_array)
            .expect("searchResults");
        assert_eq!(results.len(), 3);
        let calendar = fixture_json("p1b/calendar.json");
        let months = calendar
            .pointer("/data/merlin/pdpAvailabilityCalendar/calendarMonths")
            .and_then(Value::as_array)
            .expect("calendarMonths");
        assert_eq!(months.len(), 2);
    }

    #[test]
    fn niobe_page_wraps_payloads_like_airbnb() {
        let payload = serde_json::json!({"data": {"x": 1}});
        let html = niobe_page("niobeMinimalClientData", &[("StaysSearch:{}", &payload)]);
        assert!(html.contains(r#"<script id="data-deferred-state-0" type="application/json">"#));
        assert!(
            html.contains(r#"{"niobeMinimalClientData":[["StaysSearch:{}",{"data":{"x":1}}]]}"#)
        );
    }
}
