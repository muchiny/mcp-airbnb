//! Helpers shared by the integration tests that talk to the real MCP server
//! over an in-memory duplex transport (`mod common;` in each test file).

#![allow(dead_code)]

use std::sync::Arc;

use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};

/// A client that advertises no capabilities; enough to drive the server.
#[derive(Debug, Clone, Default)]
pub struct DummyClient;

impl ClientHandler for DummyClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

/// A connected rmcp client plus the task that runs the server side.
pub struct Connected {
    pub client: RunningService<RoleClient, DummyClient>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Connected {
    /// Cancel the client and wait for the server task to finish.
    pub async fn shutdown(self) {
        let _ = self.client.cancel().await;
        let _ = self.server.await;
    }
}

/// Start `AirbnbMcpServer` over `backend` and complete the MCP handshake.
pub async fn connect(backend: Arc<dyn AirbnbClient>) -> Connected {
    let (server_transport, client_transport) = tokio::io::duplex(1 << 20);
    let server = AirbnbMcpServer::new(backend);
    let server = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = DummyClient
        .serve(client_transport)
        .await
        .expect("the MCP initialize handshake should succeed");
    Connected { client, server }
}

/// Build `tools/call` parameters from a JSON object literal.
pub fn tool_call(name: &str, args: serde_json::Value) -> CallToolRequestParams {
    match args {
        serde_json::Value::Object(map) => {
            CallToolRequestParams::new(name.to_string()).with_arguments(map)
        }
        other => panic!("tool arguments must be a JSON object, got {other}"),
    }
}

/// Concatenate every text block of a tool result.
pub fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|content| content.raw.as_text())
        .map(|text| text.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
