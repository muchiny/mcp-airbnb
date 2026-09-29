//! End-to-end checks of the MCP resource contract over an in-process rmcp
//! client/server pair: percent-encoded URIs that match the advertised
//! templates, `text/plain` contents, tolerant reads, and (Task 8)
//! `notifications/resources/list_changed`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientInfo, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{MaybeSendFuture, NotificationContext, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};

use mcp_airbnb::domain::analytics::{HostProfile, NeighborhoodStats, OccupancyEstimate};
use mcp_airbnb::domain::calendar::PriceCalendar;
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::{AirbnbError, Result};
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

struct ResourceMock;

fn listing(id: &str) -> Listing {
    Listing {
        id: id.into(),
        name: format!("Flat {id}"),
        location: "Paris, France".into(),
        price_per_night: 120.0,
        currency: "€".into(),
        rating: Some(4.8),
        review_count: 12,
        thumbnail_url: None,
        property_type: Some("Apartment".into()),
        host_name: None,
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

fn detail(id: &str) -> ListingDetail {
    ListingDetail {
        id: id.into(),
        name: format!("Flat {id}"),
        location: "Paris, France".into(),
        description: "Bright flat near the river.".into(),
        price_per_night: 0.0,
        currency: "€".into(),
        rating: Some(4.8),
        review_count: 12,
        property_type: Some("Apartment".into()),
        host_name: Some("Host A".into()),
        url: format!("https://www.airbnb.com/rooms/{id}"),
        amenities: vec!["Wifi".into()],
        house_rules: vec![],
        latitude: None,
        longitude: None,
        photos: vec![],
        bedrooms: Some(1),
        beds: Some(1),
        bathrooms: Some(1.0),
        max_guests: Some(2),
        check_in_time: None,
        check_out_time: None,
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

fn unused<T>(what: &str) -> Result<T> {
    Err(AirbnbError::Parse {
        reason: format!("{what} is not used by these tests"),
    })
}

#[async_trait]
impl AirbnbClient for ResourceMock {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        let mut first = listing("1");
        first.location.clone_from(&params.location);
        Ok(SearchResult {
            listings: vec![first, listing("2")],
            total_count: Some(2),
            next_cursor: None,
        })
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        Ok(detail(id))
    }

    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage> {
        let next_cursor = if cursor.is_none() {
            Some("24".to_string())
        } else {
            None
        };
        Ok(ReviewsPage {
            listing_id: id.into(),
            summary: None,
            reviews: vec![Review {
                author: "Guest A".into(),
                date: "2026-08-01".into(),
                rating: Some(5.0),
                comment: "Quiet and clean.".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            }],
            next_cursor,
        })
    }

    async fn get_price_calendar(&self, _id: &str, _months: u32) -> Result<PriceCalendar> {
        unused("calendar")
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        unused("host profile")
    }

    async fn get_neighborhood_stats(&self, _params: &SearchParams) -> Result<NeighborhoodStats> {
        unused("neighborhood stats")
    }

    async fn get_occupancy_estimate(&self, _id: &str, _months: u32) -> Result<OccupancyEstimate> {
        unused("occupancy")
    }
}

#[derive(Clone, Default)]
struct CountingClient {
    list_changed: Arc<AtomicUsize>,
}

impl ClientHandler for CountingClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + MaybeSendFuture + '_ {
        self.list_changed.fetch_add(1, Ordering::SeqCst);
        std::future::ready(())
    }
}

type Client = RunningService<RoleClient, CountingClient>;

async fn connect() -> (
    Client,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    let server = AirbnbMcpServer::new(Arc::new(ResourceMock));
    let handle = tokio::spawn(async move {
        server.serve(server_io).await?.waiting().await?;
        anyhow::Ok(())
    });
    let counter = Arc::new(AtomicUsize::new(0));
    let client = CountingClient {
        list_changed: Arc::clone(&counter),
    }
    .serve(client_io)
    .await
    .expect("client connects");
    (client, counter, handle)
}

async fn disconnect(client: Client, handle: tokio::task::JoinHandle<anyhow::Result<()>>) {
    let _ = client.cancel().await;
    let _ = handle.await;
}

async fn call(client: &Client, tool: &str, args: serde_json::Value) -> CallToolResult {
    // Destructure instead of `args.as_object()`: taking `args` by value and
    // only borrowing it would trip clippy::needless_pass_by_value (pedantic).
    let serde_json::Value::Object(arguments) = args else {
        panic!("tool arguments must be a JSON object");
    };
    client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(arguments))
        .await
        .expect("tools/call transport succeeds")
}

async fn listed_uris(client: &Client) -> Vec<String> {
    client
        .peer()
        .list_all_resources()
        .await
        .expect("resources/list")
        .into_iter()
        .map(|r| r.raw.uri)
        .collect()
}

fn text_of(contents: &ResourceContents) -> (&str, Option<&str>) {
    match contents {
        ResourceContents::TextResourceContents {
            text, mime_type, ..
        } => (text.as_str(), mime_type.as_deref()),
        ResourceContents::BlobResourceContents { .. } => panic!("expected text contents"),
    }
}

#[tokio::test]
async fn search_resource_uri_is_percent_encoded_and_readable() {
    let (client, _, handle) = connect().await;
    let result = call(
        &client,
        "airbnb_search",
        serde_json::json!({ "location": "Paris, France" }),
    )
    .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");

    let uris = listed_uris(&client).await;
    let expected = "airbnb://search/Paris%2C%20France";
    assert!(uris.iter().any(|u| u == expected), "stored URIs: {uris:?}");

    let read = client
        .peer()
        .read_resource(ReadResourceRequestParams::new(expected))
        .await
        .expect("resources/read");
    let (text, mime) = text_of(&read.contents[0]);
    assert_eq!(mime, Some("text/plain"));
    assert!(text.contains("Flat 1"), "{text}");
    disconnect(client, handle).await;
}

#[tokio::test]
async fn read_resource_accepts_equivalent_encodings() {
    let (client, _, handle) = connect().await;
    let _ = call(
        &client,
        "airbnb_search",
        serde_json::json!({ "location": "São Paulo, Brasil" }),
    )
    .await;
    for spelling in [
        "airbnb://search/S%C3%A3o%20Paulo%2C%20Brasil",
        "airbnb://search/S%c3%a3o%20Paulo%2c%20Brasil",
        "airbnb://search/São Paulo%2C Brasil",
    ] {
        let read = client
            .peer()
            .read_resource(ReadResourceRequestParams::new(spelling))
            .await;
        assert!(read.is_ok(), "{spelling}: {read:?}");
    }
    disconnect(client, handle).await;
}

#[tokio::test]
async fn reviews_resource_uri_carries_the_cursor() {
    let (client, _, handle) = connect().await;
    let _ = call(&client, "airbnb_reviews", serde_json::json!({ "id": "42" })).await;
    let _ = call(
        &client,
        "airbnb_reviews",
        serde_json::json!({ "id": "42", "cursor": "24" }),
    )
    .await;
    let uris = listed_uris(&client).await;
    assert!(
        uris.iter().any(|u| u == "airbnb://listing/42/reviews"),
        "{uris:?}"
    );
    assert!(
        uris.iter()
            .any(|u| u == "airbnb://listing/42/reviews?cursor=24"),
        "{uris:?}"
    );
    disconnect(client, handle).await;
}

#[tokio::test]
async fn eighteen_text_plain_templates_are_advertised() {
    let (client, _, handle) = connect().await;
    let templates = client
        .peer()
        .list_resource_templates(None)
        .await
        .expect("resources/templates/list")
        .resource_templates;
    assert_eq!(templates.len(), 18);
    for t in &templates {
        assert_eq!(
            t.raw.mime_type.as_deref(),
            Some("text/plain"),
            "{}",
            t.raw.uri_template
        );
    }
    assert!(templates.iter().any(|t| t.raw.uri_template
        == "airbnb://search/{location}{?checkin,checkout,adults,children,infants,pets,min_price,max_price,property_type,cursor}"));
    assert!(
        templates
            .iter()
            .any(|t| t.raw.uri_template == "airbnb://analysis/price-trends/{id}{?months}")
    );
    disconnect(client, handle).await;
}

async fn wait_for(counter: &AtomicUsize, expected: usize) -> usize {
    for _ in 0..200 {
        let seen = counter.load(Ordering::SeqCst);
        if seen >= expected {
            return seen;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    counter.load(Ordering::SeqCst)
}

#[tokio::test]
async fn a_new_resource_sends_list_changed_once() {
    let (client, counter, handle) = connect().await;
    let _ = call(
        &client,
        "airbnb_listing_details",
        serde_json::json!({ "id": "42" }),
    )
    .await;
    assert_eq!(wait_for(&counter, 1).await, 1, "a new URI must notify");
    let _ = call(
        &client,
        "airbnb_listing_details",
        serde_json::json!({ "id": "42" }),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "the same URI again must not notify"
    );
    let _ = call(
        &client,
        "airbnb_listing_details",
        serde_json::json!({ "id": "43" }),
    )
    .await;
    assert_eq!(
        wait_for(&counter, 2).await,
        2,
        "another new URI must notify"
    );
    disconnect(client, handle).await;
}

#[tokio::test]
async fn capabilities_advertise_resource_list_changed() {
    let (client, _, handle) = connect().await;
    let info = client.peer_info().expect("server info");
    let resources = info
        .capabilities
        .resources
        .as_ref()
        .expect("resources capability");
    assert_eq!(resources.list_changed, Some(true));
    disconnect(client, handle).await;
}
