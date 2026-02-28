use async_trait::async_trait;

use crate::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use crate::domain::calendar::PriceCalendar;
use crate::domain::listing::{ListingDetail, SearchResult};
use crate::domain::review::ReviewsPage;
use crate::domain::search_params::SearchParams;
use crate::error::Result;

#[async_trait]
pub trait AirbnbClient: Send + Sync {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage>;
    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar>;

    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile>;
    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats>;
    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate>;
}
