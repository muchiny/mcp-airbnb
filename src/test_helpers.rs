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
        average_available_price: None,
        weekend_avg_price: None,
        weekday_avg_price: None,
        monthly_breakdown: vec![],
    }
}
