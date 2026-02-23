use async_trait::async_trait;

use crate::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};

#[async_trait]
pub trait AirbnbClient: Send + Sync {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage>;
    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar>;

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Err(AirbnbError::Parse {
            reason: "get_host_profile not implemented".into(),
        })
    }

    async fn get_neighborhood_stats(&self, _params: &SearchParams) -> Result<NeighborhoodStats> {
        Err(AirbnbError::Parse {
            reason: "get_neighborhood_stats not implemented".into(),
        })
    }

    async fn get_occupancy_estimate(&self, _id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Err(AirbnbError::Parse {
            reason: "get_occupancy_estimate not implemented".into(),
        })
    }
}
