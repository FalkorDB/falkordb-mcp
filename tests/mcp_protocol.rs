//! In-process MCP protocol test: a real `rmcp` client talks to `FalkorMcp` over an in-memory
//! duplex stream, exercising the full tool-discovery + dispatch + serialization path — no database.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{model::CallToolRequestParams, ServiceExt};
use serde_json::json;

use falkordb_mcp::backend::{FalkorBackend, QueryOutput, Schema};
use falkordb_mcp::server::{FalkorMcp, DEFAULT_MAX_ROWS};

/// A minimal canned backend (the lib's `FakeBackend` is `#[cfg(test)]` and not visible here).
struct StubBackend;

#[async_trait]
impl FalkorBackend for StubBackend {
    async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec!["social".into()])
    }
    async fn schema(
        &self,
        _graph: &str,
    ) -> anyhow::Result<Schema> {
        Ok(Schema::default())
    }
    async fn read_query(
        &self,
        _graph: &str,
        _cypher: &str,
        _params: BTreeMap<String, serde_json::Value>,
        _limit: usize,
    ) -> anyhow::Result<QueryOutput> {
        Ok(QueryOutput::default())
    }
    async fn explain(
        &self,
        _graph: &str,
        _cypher: &str,
    ) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
    async fn write_query(
        &self,
        _graph: &str,
        _cypher: &str,
        _params: BTreeMap<String, serde_json::Value>,
        _limit: usize,
    ) -> anyhow::Result<QueryOutput> {
        Ok(QueryOutput {
            columns: vec!["nodes_created".into()],
            rows: vec![vec![serde_json::json!(1)]],
            truncated: false,
        })
    }
    async fn profile(
        &self,
        _graph: &str,
        _cypher: &str,
    ) -> anyhow::Result<Vec<String>> {
        Ok(vec!["Results".into()])
    }
}

#[tokio::test]
async fn client_discovers_and_calls_tools_over_duplex() {
    let (server_io, client_io) = tokio::io::duplex(8192);

    // Serve FalkorMcp (read-only) on one end of the pipe.
    let server = FalkorMcp::new(Arc::new(StubBackend), DEFAULT_MAX_ROWS, false);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server starts");
        running.waiting().await.expect("server runs to completion");
    });

    // Drive it with a real MCP client on the other end (`()` is a no-op ClientHandler).
    let client = ().serve(client_io).await.expect("client connects");

    // All four read-only tools are advertised.
    let tools = client.list_all_tools().await.expect("list tools");
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["explain", "get_schema", "list_graphs", "query_read"]
    );

    // Calling a tool routes through the server and returns its JSON payload.
    let result = client
        .call_tool(CallToolRequestParams::new("list_graphs"))
        .await
        .expect("call list_graphs");
    let envelope = serde_json::to_value(&result).expect("result serializes");
    let text = envelope["content"][0]["text"]
        .as_str()
        .expect("text content");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).expect("payload is JSON"),
        json!(["social"]),
    );

    client.cancel().await.expect("client shuts down");
}

#[tokio::test]
async fn writable_server_exposes_and_runs_write_tools() {
    let (server_io, client_io) = tokio::io::duplex(8192);

    // Serve a write-enabled FalkorMcp.
    let server = FalkorMcp::new(Arc::new(StubBackend), DEFAULT_MAX_ROWS, true);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server starts");
        running.waiting().await.expect("server runs to completion");
    });

    let client = ().serve(client_io).await.expect("client connects");

    // The two guarded write tools are now advertised alongside the four read tools.
    let names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("list tools")
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names.len(), 6, "writes enabled → 6 tools");
    assert!(names.contains(&"query_write".to_string()));
    assert!(names.contains(&"profile".to_string()));

    // query_write routes through and returns rows.
    let request: CallToolRequestParams = serde_json::from_value(json!({
        "name": "query_write",
        "arguments": { "graph": "g", "cypher": "CREATE (:N)" }
    }))
    .expect("build request");
    let result = client.call_tool(request).await.expect("call query_write");
    let envelope = serde_json::to_value(&result).expect("result serializes");
    let payload: serde_json::Value =
        serde_json::from_str(envelope["content"][0]["text"].as_str().expect("text"))
            .expect("payload is JSON");
    assert_eq!(payload["columns"], json!(["nodes_created"]));

    client.cancel().await.expect("client shuts down");
}
