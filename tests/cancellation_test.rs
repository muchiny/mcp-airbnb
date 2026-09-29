//! MCP-8: a cancelled tool call must stop its upstream work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use rmcp::model::{CallToolRequest, CallToolRequestParams, ClientInfo, ClientRequest};
use rmcp::service::PeerRequestOptions;
use rmcp::{ClientHandler, ServiceExt};
use tokio::sync::Notify;

use mcp_airbnb::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use mcp_airbnb::domain::calendar::PriceCalendar;
use mcp_airbnb::domain::listing::{ListingDetail, SearchResult};
use mcp_airbnb::domain::review::ReviewsPage;
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::{AirbnbError, Result};
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

#[derive(Debug, Clone, Default)]
struct DummyClient;

impl ClientHandler for DummyClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

/// Sets its flag when dropped, which proves the in-flight future was dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Upstream whose listing-detail call never completes.
struct HangingClient {
    started: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}

fn not_used() -> AirbnbError {
    AirbnbError::Parse {
        reason: "not used in this test".into(),
    }
}

#[async_trait]
impl AirbnbClient for HangingClient {
    async fn search_listings(&self, _params: &SearchParams) -> Result<SearchResult> {
        Err(not_used())
    }

    async fn get_listing_detail(&self, _id: &str) -> Result<ListingDetail> {
        let _guard = DropFlag(Arc::clone(&self.dropped));
        self.started.notify_one();
        std::future::pending::<Result<ListingDetail>>().await
    }

    async fn get_reviews(&self, _id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Err(not_used())
    }

    async fn get_price_calendar(&self, _id: &str, _months: u32) -> Result<PriceCalendar> {
        Err(not_used())
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Err(not_used())
    }

    async fn get_neighborhood_stats(&self, _params: &SearchParams) -> Result<NeighborhoodStats> {
        Err(not_used())
    }

    async fn get_occupancy_estimate(&self, _id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Err(not_used())
    }
}

#[tokio::test]
async fn cancelled_tool_call_drops_the_upstream_future() {
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let upstream = HangingClient {
        started: Arc::clone(&started),
        dropped: Arc::clone(&dropped),
    };

    let (server_transport, client_transport) = tokio::io::duplex(65_536);
    let server = AirbnbMcpServer::new(Arc::new(upstream));
    let server_handle = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = DummyClient
        .serve(client_transport)
        .await
        .expect("client connects");

    let arguments = serde_json::json!({ "id": "12345" })
        .as_object()
        .expect("object")
        .clone();
    let params = CallToolRequestParams::new("airbnb_listing_details").with_arguments(arguments);
    let handle = client
        .peer()
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(params)),
            PeerRequestOptions::no_options(),
        )
        .await
        .expect("request sent");

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("the tool reached the upstream client");
    assert!(
        !dropped.load(Ordering::SeqCst),
        "upstream future dropped before cancellation"
    );

    handle
        .cancel(Some("user gave up".into()))
        .await
        .expect("cancellation sent");

    tokio::time::timeout(Duration::from_secs(5), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the in-flight upstream future must be dropped after notifications/cancelled");

    let _ = client.cancel().await;
    let _ = server_handle.await;
}
