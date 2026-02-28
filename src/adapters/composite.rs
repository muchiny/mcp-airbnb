use async_trait::async_trait;
use tracing::warn;

use crate::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::Result;
use crate::ports::airbnb_client::AirbnbClient;

/// A client that tries the GraphQL API first and falls back to HTML scraping.
pub struct CompositeClient {
    graphql: Box<dyn AirbnbClient>,
    scraper: Box<dyn AirbnbClient>,
}

impl CompositeClient {
    pub fn new(graphql: Box<dyn AirbnbClient>, scraper: Box<dyn AirbnbClient>) -> Self {
        Self { graphql, scraper }
    }
}

/// Try the primary implementation, fall back to secondary on error.
macro_rules! with_fallback {
    ($self:expr, $method:ident $(, $arg:expr)*) => {{
        match $self.graphql.$method($($arg),*).await {
            Ok(result) => Ok(result),
            Err(e) => {
                warn!(
                    error = %e,
                    method = stringify!($method),
                    "GraphQL failed, falling back to HTML scraper"
                );
                $self.scraper.$method($($arg),*).await
            }
        }
    }};
}

#[async_trait]
impl AirbnbClient for CompositeClient {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        with_fallback!(self, search_listings, params)
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        match self.graphql.get_listing_detail(id).await {
            Ok(mut gql) => {
                // If GraphQL result is missing critical fields, try to fill from scraper
                if (gql.name.is_empty()
                    || gql.location.is_empty()
                    || gql.amenities.is_empty()
                    || gql.description.is_empty()
                    || gql.photos.is_empty()
                    || gql.house_rules.is_empty()
                    || gql.price_per_night == 0.0
                    || gql.rating.is_none())
                    && let Ok(scraped) = self.scraper.get_listing_detail(id).await
                {
                    if gql.name.is_empty() && !scraped.name.is_empty() {
                        gql.name = scraped.name;
                    }
                    if gql.location.is_empty() && !scraped.location.is_empty() {
                        gql.location = scraped.location;
                    }
                    if gql.description.is_empty() && !scraped.description.is_empty() {
                        gql.description = scraped.description;
                    }
                    if gql.amenities.is_empty() && !scraped.amenities.is_empty() {
                        gql.amenities = scraped.amenities;
                    }
                    if gql.photos.is_empty() && !scraped.photos.is_empty() {
                        gql.photos = scraped.photos;
                    }
                    if gql.house_rules.is_empty() && !scraped.house_rules.is_empty() {
                        gql.house_rules = scraped.house_rules;
                    }
                    if gql.host_name.is_none() {
                        gql.host_name = scraped.host_name;
                    }
                    if gql.price_per_night == 0.0 && scraped.price_per_night > 0.0 {
                        gql.price_per_night = scraped.price_per_night;
                        gql.currency = scraped.currency;
                    }
                    if gql.rating.is_none() {
                        gql.rating = scraped.rating;
                    }
                    if gql.review_count == 0 {
                        gql.review_count = scraped.review_count;
                    }
                    if gql.host_id.is_none() {
                        gql.host_id = scraped.host_id;
                    }
                }
                Ok(gql)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    method = "get_listing_detail",
                    "GraphQL failed, falling back to HTML scraper"
                );
                self.scraper.get_listing_detail(id).await
            }
        }
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        match self.graphql.get_reviews(id, cursor).await {
            Ok(gql_page) => {
                // If GraphQL returned summary but no individual reviews, try scraper
                if gql_page.reviews.is_empty()
                    && let Ok(scraped) = self.scraper.get_reviews(id, cursor).await
                {
                    if !scraped.reviews.is_empty() {
                        return Ok(scraped);
                    }
                    // If scraper also has no reviews but has a better summary, merge
                    if gql_page.summary.is_none() && scraped.summary.is_some() {
                        return Ok(scraped);
                    }
                }
                Ok(gql_page)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    method = "get_reviews",
                    "GraphQL failed, falling back to HTML scraper"
                );
                self.scraper.get_reviews(id, cursor).await
            }
        }
    }

    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar> {
        with_fallback!(self, get_price_calendar, id, months)
    }

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile> {
        with_fallback!(self, get_host_profile, listing_id)
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        with_fallback!(self, get_neighborhood_stats, params)
    }

    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate> {
        with_fallback!(self, get_occupancy_estimate, id, months)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AirbnbError;
    use crate::test_helpers::*;

    fn make_composite(graphql: MockAirbnbClient, scraper: MockAirbnbClient) -> CompositeClient {
        CompositeClient::new(Box::new(graphql), Box::new(scraper))
    }

    #[tokio::test]
    async fn graphql_success_no_fallback() {
        let gql = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "1",
                "GQL Result",
                100.0,
            )]))
        });
        // Scraper returns error — should never be called
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Paris".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].name, "GQL Result");
    }

    #[tokio::test]
    async fn graphql_error_falls_back_to_scraper() {
        let gql = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "2",
                "Scraper Result",
                200.0,
            )]))
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Paris".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings[0].name, "Scraper Result");
    }

    #[tokio::test]
    async fn both_fail_returns_scraper_error() {
        let gql = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Err(AirbnbError::Parse {
                reason: "scraper failed".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Paris".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let err = composite.search_listings(&params).await.unwrap_err();
        assert!(err.to_string().contains("scraper failed"));
    }

    #[tokio::test]
    async fn fallback_search() {
        let gql = MockAirbnbClient::new().with_search(|_| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_search(|_| {
            Ok(make_search_result(vec![make_listing(
                "1", "Fallback", 50.0,
            )]))
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Tokyo".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let result = composite.search_listings(&params).await.unwrap();
        assert_eq!(result.listings.len(), 1);
    }

    #[tokio::test]
    async fn fallback_detail() {
        let gql = MockAirbnbClient::new().with_detail(|_| {
            Err(AirbnbError::Parse {
                reason: "gql fail".into(),
            })
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraped Detail".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Scraped Detail");
    }

    #[tokio::test]
    async fn fallback_reviews() {
        let gql = MockAirbnbClient::new().with_reviews(|_, _| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new()
            .with_reviews(|id, _| Ok(make_reviews_page(id, vec![make_review("Alice", "Great!")])));
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Alice");
    }

    #[tokio::test]
    async fn fallback_calendar() {
        let gql = MockAirbnbClient::new().with_calendar(|_, _| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_calendar(|id, _| {
            Ok(make_price_calendar(
                id,
                vec![make_calendar_day("2025-06-01", Some(100.0), true)],
            ))
        });
        let composite = make_composite(gql, scraper);
        let cal = composite.get_price_calendar("42", 3).await.unwrap();
        assert_eq!(cal.days.len(), 1);
    }

    #[tokio::test]
    async fn fallback_host_profile() {
        let gql = MockAirbnbClient::new().with_host_profile(|_| Err(AirbnbError::RateLimited));
        let scraper =
            MockAirbnbClient::new().with_host_profile(|_| Ok(make_host_profile("Scraped Host")));
        let composite = make_composite(gql, scraper);
        let profile = composite.get_host_profile("42").await.unwrap();
        assert_eq!(profile.name, "Scraped Host");
    }

    #[tokio::test]
    async fn fallback_neighborhood_stats() {
        let gql = MockAirbnbClient::new().with_neighborhood(|_| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_neighborhood(|params| {
            let mut stats = make_neighborhood_stats(&params.location);
            stats.total_listings = 42;
            Ok(stats)
        });
        let composite = make_composite(gql, scraper);
        let params = SearchParams {
            location: "Berlin".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        let stats = composite.get_neighborhood_stats(&params).await.unwrap();
        assert_eq!(stats.total_listings, 42);
    }

    #[tokio::test]
    async fn fallback_occupancy_estimate() {
        let gql = MockAirbnbClient::new().with_occupancy(|_, _| Err(AirbnbError::RateLimited));
        let scraper = MockAirbnbClient::new().with_occupancy(|id, _| {
            let mut est = make_occupancy_estimate(id);
            est.occupancy_rate = 0.75;
            Ok(est)
        });
        let composite = make_composite(gql, scraper);
        let est = composite.get_occupancy_estimate("42", 3).await.unwrap();
        assert!((est.occupancy_rate - 0.75).abs() < f64::EPSILON);
    }

    // --- Detail smart merge tests ---

    /// Helper: creates a GQL detail with specific empty fields to trigger merge
    fn gql_detail_with_empty_name(id: &str) -> ListingDetail {
        let mut d = make_listing_detail(id);
        d.name = String::new(); // triggers merge condition
        d
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_name() {
        let gql = MockAirbnbClient::new().with_detail(|id| Ok(gql_detail_with_empty_name(id)));
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraped Name".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Scraped Name");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_location() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.location = String::new(); // triggers merge condition
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.location = "Scraped City".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.location, "Scraped City");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_amenities() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.amenities = vec![]; // triggers merge condition
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.amenities = vec!["Pool".into(), "Sauna".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.amenities, vec!["Pool", "Sauna"]);
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_description() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.description = String::new();
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.description = "Scraped description".into();
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.description, "Scraped description");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_photos() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.photos = vec![];
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.photos = vec!["https://example.com/photo.jpg".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.photos, vec!["https://example.com/photo.jpg"]);
    }

    #[tokio::test]
    async fn detail_merge_fills_missing_host_name() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge
            d.host_name = None;
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.host_name = Some("Scraped Host".into());
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.host_name, Some("Scraped Host".into()));
    }

    #[tokio::test]
    async fn detail_merge_preserves_nonempty_fields() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "GQL Name".into();
            d.location = "GQL City".into();
            d.description = "GQL desc".into();
            d.amenities = vec!["WiFi".into()];
            d.photos = vec!["gql_photo.jpg".into()];
            d.host_name = Some("GQL Host".into());
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Scraper Name".into();
            d.location = "Scraper City".into();
            d.description = "Scraper desc".into();
            d.amenities = vec!["Pool".into()];
            d.photos = vec!["scraper_photo.jpg".into()];
            d.host_name = Some("Scraper Host".into());
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        // All GQL values preserved — merge condition not triggered
        assert_eq!(detail.name, "GQL Name");
        assert_eq!(detail.location, "GQL City");
        assert_eq!(detail.description, "GQL desc");
        assert_eq!(detail.amenities, vec!["WiFi"]);
        assert_eq!(detail.photos, vec!["gql_photo.jpg"]);
        assert_eq!(detail.host_name, Some("GQL Host".into()));
    }

    #[tokio::test]
    async fn detail_merge_skips_when_all_critical_present() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = "Present".into();
            d.location = "Present".into();
            d.description = "Present desc".into();
            d.amenities = vec!["Present".into()];
            d.photos = vec!["photo.jpg".into()];
            d.house_rules = vec!["No parties".into()];
            d.price_per_night = 100.0;
            d.rating = Some(4.5);
            Ok(d)
        });
        // Scraper returns error — should never be called since merge condition not met
        let scraper = MockAirbnbClient::new().with_detail(|_| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.name, "Present");
    }

    #[tokio::test]
    async fn detail_merge_fills_empty_house_rules() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.house_rules = vec![]; // triggers merge condition
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.house_rules = vec!["No smoking".into(), "No pets".into()];
            Ok(d)
        });
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        assert_eq!(detail.house_rules, vec!["No smoking", "No pets"]);
    }

    #[tokio::test]
    async fn detail_merge_scraper_error_returns_gql_only() {
        let gql = MockAirbnbClient::new().with_detail(|id| {
            let mut d = make_listing_detail(id);
            d.name = String::new(); // trigger merge attempt
            d.location = "GQL City".into();
            Ok(d)
        });
        let scraper = MockAirbnbClient::new().with_detail(|_| Err(AirbnbError::RateLimited));
        let composite = make_composite(gql, scraper);
        let detail = composite.get_listing_detail("42").await.unwrap();
        // GQL result returned as-is since scraper failed
        assert!(detail.name.is_empty());
        assert_eq!(detail.location, "GQL City");
    }

    // --- Reviews smart merge tests ---

    #[tokio::test]
    async fn reviews_merge_gql_empty_uses_scraper_reviews() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(id, vec![])) // empty reviews
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("Alice", "Great place!")],
            ))
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Alice");
    }

    #[tokio::test]
    async fn reviews_merge_gql_empty_no_summary_uses_scraper_summary() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            // GQL: empty reviews, no summary
            Ok(make_reviews_page(id, vec![]))
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            // Scraper: empty reviews but has summary
            let mut page = make_reviews_page(id, vec![]);
            page.summary = Some(make_reviews_summary());
            Ok(page)
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.summary.is_some());
        assert!((page.summary.unwrap().overall_rating - 4.7).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn reviews_merge_gql_has_reviews_ignores_scraper() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(
                id,
                vec![make_review("GQL Author", "GQL review")],
            ))
        });
        // Scraper should not be used — error proves it's never called
        let scraper = MockAirbnbClient::new().with_reviews(|_, _| {
            Err(AirbnbError::Parse {
                reason: "should not be called".into(),
            })
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "GQL Author");
    }

    #[tokio::test]
    async fn reviews_merge_both_empty_returns_gql() {
        let gql = MockAirbnbClient::new().with_reviews(|id, _| {
            let mut page = make_reviews_page(id, vec![]);
            page.summary = Some(make_reviews_summary()); // GQL has summary
            Ok(page)
        });
        let scraper = MockAirbnbClient::new().with_reviews(|id, _| {
            Ok(make_reviews_page(id, vec![])) // scraper also empty, no summary
        });
        let composite = make_composite(gql, scraper);
        let page = composite.get_reviews("42", None).await.unwrap();
        assert!(page.reviews.is_empty());
        // GQL result returned because scraper has no reviews and no better summary
        assert!(page.summary.is_some());
    }
}
