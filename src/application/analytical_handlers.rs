//! Orchestration helpers for analytical commands that need multiple
//! `AirbnbClient` fetches before calling a `domain::analytics::compute_*`
//! function. Used by the CLI; mirror of the logic in `src/mcp/server.rs`
//! for the same tools, but without the MCP resource side effects.

use std::sync::Arc;

use crate::domain::analytics::{
    AmenityAnalysis, CompareListingsResult, CompetitivePositioning, HostPortfolio,
    PricingRecommendation, RevenueEstimate, compute_amenity_analysis, compute_compare_listings,
    compute_competitive_positioning, compute_host_portfolio, compute_optimal_pricing,
    compute_price_trends, compute_revenue_estimate,
};
use crate::domain::listing::Listing;
use crate::domain::search_params::SearchParams;
use crate::error::{AirbnbError, Result};
use crate::ports::airbnb_client::AirbnbClient;

/// Compare listings: either by explicit IDs (up to 10) or by searching a
/// location (up to 20). At least one of `ids` or `location` must be provided.
pub async fn run_compare_listings(
    client: Arc<dyn AirbnbClient>,
    ids: Option<Vec<String>>,
    location: Option<String>,
) -> Result<CompareListingsResult> {
    if ids.is_none() && location.is_none() {
        return Err(AirbnbError::InvalidParams {
            reason: "provide either --ids or --location".into(),
        });
    }

    let listings: Vec<Listing> = if let Some(ref ids) = ids {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids.iter().take(10) {
            let detail = client.get_listing_detail(id).await?;
            out.push(detail_to_listing(detail));
        }
        out
    } else {
        let loc = location.unwrap_or_default();
        let sp = SearchParams {
            location: loc,
            ..SearchParams::default()
        };
        let result = client.search_listings(&sp).await?;
        result.listings.into_iter().take(20).collect()
    };

    if listings.len() < 2 {
        return Err(AirbnbError::InvalidParams {
            reason: "need at least 2 listings to compare".into(),
        });
    }

    Ok(compute_compare_listings(&listings, None))
}

/// Revenue estimate: combines calendar, occupancy, and neighborhood stats.
pub async fn run_revenue_estimate(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
    months: u32,
) -> Result<RevenueEstimate> {
    // Anchor on the listing detail — gives us the location if none was provided.
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let calendar = client.get_price_calendar(id, months).await.ok();
    let occupancy = client.get_occupancy_estimate(id, months).await.ok();
    let neighborhood = {
        let sp = SearchParams {
            location: location.clone(),
            ..SearchParams::default()
        };
        client.get_neighborhood_stats(&sp).await.ok()
    };

    Ok(compute_revenue_estimate(
        Some(id),
        &location,
        calendar.as_ref(),
        neighborhood.as_ref(),
        occupancy.as_ref(),
    ))
}

/// Amenity analysis: compares a listing's amenities against a sample of
/// neighbour listings fetched via search + per-listing details.
pub async fn run_amenity_analysis(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
) -> Result<AmenityAnalysis> {
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location,
        ..SearchParams::default()
    };
    let neighbor_ids: Vec<String> = match client.search_listings(&sp).await {
        Ok(result) => result
            .listings
            .into_iter()
            .filter(|l| l.id != id)
            .take(5)
            .map(|l| l.id)
            .collect(),
        Err(_) => Vec::new(),
    };

    let mut neighbor_details = Vec::new();
    for nid in &neighbor_ids {
        if let Ok(d) = client.get_listing_detail(nid).await {
            neighbor_details.push(d);
        }
    }

    Ok(compute_amenity_analysis(&detail, &neighbor_details))
}

/// Host portfolio: fetches the listing's host info, then searches for other
/// listings by the same host (filtered by `host_id` when available, else by
/// `host_name`). Falls back to a single-listing portfolio if no siblings
/// are found.
pub async fn run_host_portfolio(
    client: Arc<dyn AirbnbClient>,
    listing_id: &str,
) -> Result<HostPortfolio> {
    let detail = client.get_listing_detail(listing_id).await?;
    let host_name = detail
        .host_name
        .clone()
        .unwrap_or_else(|| "Unknown Host".to_string());
    let host_id = detail.host_id.clone();
    let is_superhost = detail.host_is_superhost;

    let sp = SearchParams {
        location: detail.location.clone(),
        ..SearchParams::default()
    };
    let host_listings: Vec<Listing> = match client.search_listings(&sp).await {
        Ok(result) => {
            let all = result.listings;
            if let Some(ref hid) = host_id {
                let by_id: Vec<_> = all
                    .iter()
                    .filter(|l| l.host_id.as_deref() == Some(hid.as_str()))
                    .cloned()
                    .collect();
                if by_id.is_empty() {
                    all.into_iter()
                        .filter(|l| l.host_name.as_deref() == detail.host_name.as_deref())
                        .collect()
                } else {
                    by_id
                }
            } else {
                all.into_iter()
                    .filter(|l| l.host_name.as_deref() == detail.host_name.as_deref())
                    .collect()
            }
        }
        Err(_) => Vec::new(),
    };

    let listings = if host_listings.is_empty() {
        vec![detail_to_listing(detail)]
    } else {
        host_listings
    };

    Ok(compute_host_portfolio(
        &host_name,
        host_id.as_deref(),
        is_superhost,
        &listings,
    ))
}

/// Competitive positioning: 5-axis scoring (price, rating, amenities, reviews,
/// occupancy) against neighborhood aggregates.
pub async fn run_competitive_positioning(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
) -> Result<CompetitivePositioning> {
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location: location.clone(),
        ..SearchParams::default()
    };
    let neighborhood = client.get_neighborhood_stats(&sp).await?;
    let occupancy = client.get_occupancy_estimate(id, 3).await.ok();

    let amenity_analysis = match client.search_listings(&sp).await {
        Ok(search_result) => {
            let neighbor_ids: Vec<String> = search_result
                .listings
                .into_iter()
                .filter(|l| l.id != id)
                .take(5)
                .map(|l| l.id)
                .collect();
            let mut neighbor_details = Vec::new();
            for nid in &neighbor_ids {
                if let Ok(d) = client.get_listing_detail(nid).await {
                    neighbor_details.push(d);
                }
            }
            Some(compute_amenity_analysis(&detail, &neighbor_details))
        }
        Err(_) => None,
    };

    Ok(compute_competitive_positioning(
        &detail,
        &neighborhood,
        occupancy.as_ref(),
        amenity_analysis.as_ref(),
    ))
}

/// Optimal pricing: combines neighborhood stats, price trends, and amenity
/// analysis to produce a recommendation with reasoning.
pub async fn run_optimal_pricing(
    client: Arc<dyn AirbnbClient>,
    id: &str,
    location: Option<String>,
    months: u32,
) -> Result<PricingRecommendation> {
    let detail = client.get_listing_detail(id).await?;
    let location = location.unwrap_or_else(|| detail.location.clone());

    let sp = SearchParams {
        location,
        ..SearchParams::default()
    };
    let neighborhood = client.get_neighborhood_stats(&sp).await.ok();
    let price_trends = match client.get_price_calendar(id, months).await {
        Ok(calendar) => Some(compute_price_trends(id, &calendar)),
        Err(_) => None,
    };

    let amenity_analysis = match client.search_listings(&sp).await {
        Ok(search_result) => {
            let neighbor_ids: Vec<String> = search_result
                .listings
                .into_iter()
                .filter(|l| l.id != id)
                .take(5)
                .map(|l| l.id)
                .collect();
            let mut neighbor_details = Vec::new();
            for nid in &neighbor_ids {
                if let Ok(d) = client.get_listing_detail(nid).await {
                    neighbor_details.push(d);
                }
            }
            Some(compute_amenity_analysis(&detail, &neighbor_details))
        }
        Err(_) => None,
    };

    Ok(compute_optimal_pricing(
        &detail,
        neighborhood.as_ref(),
        price_trends.as_ref(),
        amenity_analysis.as_ref(),
    ))
}

/// Convert a `ListingDetail` to a lightweight `Listing` for use in
/// compare-listings when we only have details (fetched by IDs). Mirrors the
/// conversion done in `src/mcp/server.rs::airbnb_compare_listings`.
fn detail_to_listing(d: crate::domain::listing::ListingDetail) -> Listing {
    Listing {
        id: d.id,
        name: d.name,
        location: d.location,
        price_per_night: d.price_per_night,
        currency: d.currency,
        rating: d.rating,
        review_count: d.review_count,
        thumbnail_url: None,
        property_type: d.property_type,
        host_name: d.host_name,
        host_id: d.host_id,
        url: d.url,
        is_superhost: d.host_is_superhost,
        is_guest_favorite: None,
        instant_book: d.instant_book,
        total_price: None,
        photos: d.photos,
        latitude: d.latitude,
        longitude: d.longitude,
    }
}
