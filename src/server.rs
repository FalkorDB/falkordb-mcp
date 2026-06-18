//! The MCP server: read-only FalkorDB tools exposed via `rmcp`.

use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;

use crate::backend::FalkorBackend;

/// Default cap on rows returned by `query_read`.
pub const DEFAULT_MAX_ROWS: usize = 1000;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GraphArg {
    /// The name of the graph.
    pub graph: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadQueryArg {
    /// The name of the graph to query.
    pub graph: String,
    /// A **read-only** Cypher query. Writes (`CREATE`/`MERGE`/`DELETE`/`SET`) are rejected by the
    /// server. Prefer adding a `LIMIT`.
    pub cypher: String,
    /// Optional query parameters, bound by name (do not string-interpolate values into `cypher`).
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// Maximum rows to return (defaults to, and is capped at, the server's row cap).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExplainArg {
    /// The name of the graph.
    pub graph: String,
    /// The Cypher query to plan (it is **not** executed).
    pub cypher: String,
}

/// The FalkorDB MCP server. Read-only in v1.
#[derive(Clone)]
pub struct FalkorMcp {
    backend: Arc<dyn FalkorBackend>,
    max_rows: usize,
    tool_router: ToolRouter<FalkorMcp>,
}

#[tool_router]
impl FalkorMcp {
    pub fn new(
        backend: Arc<dyn FalkorBackend>,
        max_rows: usize,
    ) -> Self {
        Self {
            backend,
            max_rows,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List the names of all graphs on the FalkorDB server.")]
    async fn list_graphs(&self) -> Result<CallToolResult, McpError> {
        let graphs = self.backend.list_graphs().await.map_err(internal)?;
        json_result(&graphs)
    }

    #[tool(
        description = "Get a graph's schema (labels, relationship types, property keys, indexes, \
                       constraints). Call this before writing a query so you use the real names."
    )]
    async fn get_schema(
        &self,
        Parameters(GraphArg { graph }): Parameters<GraphArg>,
    ) -> Result<CallToolResult, McpError> {
        let schema = self.backend.schema(&graph).await.map_err(internal)?;
        json_result(&schema)
    }

    #[tool(
        description = "Run a READ-ONLY Cypher query and return rows as JSON. Writes are rejected by \
                       the server. Results are capped; include a LIMIT for large graphs."
    )]
    async fn query_read(
        &self,
        Parameters(arg): Parameters<ReadQueryArg>,
    ) -> Result<CallToolResult, McpError> {
        let limit = arg.limit.unwrap_or(self.max_rows).min(self.max_rows);
        let params = arg.params.into_iter().collect();
        let out = self
            .backend
            .read_query(&arg.graph, &arg.cypher, params, limit)
            .await
            .map_err(internal)?;
        json_result(&out)
    }

    #[tool(
        description = "Return the query plan for a Cypher query WITHOUT executing it \
                       (GRAPH.EXPLAIN). Use it to spot full label scans and missing indexes."
    )]
    async fn explain(
        &self,
        Parameters(ExplainArg { graph, cypher }): Parameters<ExplainArg>,
    ) -> Result<CallToolResult, McpError> {
        let plan = self
            .backend
            .explain(&graph, &cypher)
            .await
            .map_err(internal)?;
        json_result(&plan)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FalkorMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Read-only access to a FalkorDB graph database. Use get_schema to learn the real \
                 labels/keys, query_read for data (read-only, capped), and explain for query plans."
                    .to_string(),
            )
    }
}

/// Map a backend error into an MCP error, appending a FalkorDB mitigation hint when one applies.
///
/// The hint is a fixed `&'static str` from [`falkordb::FalkorDBError::mitigation_hint`], so it never
/// echoes credentials or other dynamic text back to the client.
fn internal(err: anyhow::Error) -> McpError {
    let mut message = err.to_string();
    if let Some(hint) = err
        .chain()
        .find_map(|e| e.downcast_ref::<falkordb::FalkorDBError>())
        .and_then(falkordb::FalkorDBError::mitigation_hint)
    {
        message.push_str("\nhint: ");
        message.push_str(hint);
    }
    McpError::internal_error(message, None)
}

fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FakeBackend, QueryOutput, Schema};
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    fn server() -> FalkorMcp {
        FalkorMcp::new(Arc::new(FakeBackend), DEFAULT_MAX_ROWS)
    }

    /// Parse the JSON payload a tool returned in its first text content block.
    fn tool_json(result: &CallToolResult) -> serde_json::Value {
        let envelope = serde_json::to_value(result).expect("CallToolResult serializes");
        let text = envelope["content"][0]["text"]
            .as_str()
            .expect("first content block is text");
        serde_json::from_str(text).expect("tool output is JSON")
    }

    #[tokio::test]
    async fn list_graphs_returns_graph_names() {
        let result = server().list_graphs().await.expect("ok");
        assert_eq!(tool_json(&result), serde_json::json!(["social", "imdb"]));
    }

    #[tokio::test]
    async fn get_schema_returns_schema() {
        let result = server()
            .get_schema(Parameters(GraphArg {
                graph: "social".into(),
            }))
            .await
            .expect("ok");
        let json = tool_json(&result);
        assert_eq!(json["labels"], serde_json::json!(["Person", "Film"]));
        assert_eq!(json["relationship_types"], serde_json::json!(["ACTED_IN"]));
        assert!(json["constraints"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn query_read_returns_rows() {
        let result = server()
            .query_read(Parameters(ReadQueryArg {
                graph: "imdb".into(),
                cypher: "MATCH (p:Person) RETURN p.name AS name".into(),
                params: serde_json::Map::new(),
                limit: None,
            }))
            .await
            .expect("ok");
        let json = tool_json(&result);
        assert_eq!(json["columns"], serde_json::json!(["name"]));
        assert_eq!(json["rows"], serde_json::json!([["Keanu"]]));
        assert_eq!(json["truncated"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn explain_returns_plan() {
        let result = server()
            .explain(Parameters(ExplainArg {
                graph: "imdb".into(),
                cypher: "MATCH (n) RETURN n".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(tool_json(&result).as_array().unwrap().len(), 2);
    }

    /// Records the row cap the server forwards to the backend.
    struct RecordingBackend {
        seen_limit: Mutex<Option<usize>>,
    }

    #[async_trait]
    impl FalkorBackend for RecordingBackend {
        async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
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
            limit: usize,
        ) -> anyhow::Result<QueryOutput> {
            *self.seen_limit.lock().unwrap() = Some(limit);
            Ok(QueryOutput::default())
        }
        async fn explain(
            &self,
            _graph: &str,
            _cypher: &str,
        ) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
    }

    async fn effective_limit(
        requested: Option<usize>,
        max_rows: usize,
    ) -> usize {
        let backend = Arc::new(RecordingBackend {
            seen_limit: Mutex::new(None),
        });
        let server = FalkorMcp::new(backend.clone(), max_rows);
        server
            .query_read(Parameters(ReadQueryArg {
                graph: "g".into(),
                cypher: "MATCH (n) RETURN n".into(),
                params: serde_json::Map::new(),
                limit: requested,
            }))
            .await
            .expect("ok");
        let seen = *backend.seen_limit.lock().unwrap();
        seen.expect("read_query was called")
    }

    #[tokio::test]
    async fn query_read_defaults_and_caps_the_limit() {
        assert_eq!(
            effective_limit(None, 1000).await,
            1000,
            "None -> server cap"
        );
        assert_eq!(effective_limit(Some(10), 1000).await, 10, "below cap kept");
        assert_eq!(
            effective_limit(Some(5000), 1000).await,
            1000,
            "above cap clamped"
        );
    }

    /// Always fails with a recognizable FalkorDB error (one that has a mitigation hint).
    struct ErrorBackend;

    #[async_trait]
    impl FalkorBackend for ErrorBackend {
        async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
            Err(anyhow::Error::from(falkordb::FalkorDBError::ConnectionDown))
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
    }

    #[tokio::test]
    async fn tool_error_surfaces_mitigation_hint() {
        let server = FalkorMcp::new(Arc::new(ErrorBackend), DEFAULT_MAX_ROWS);
        let err = server.list_graphs().await.expect_err("backend errors");
        let hint = falkordb::FalkorDBError::ConnectionDown
            .mitigation_hint()
            .expect("ConnectionDown has a hint");
        assert!(
            err.message.contains(hint),
            "hint should be appended, got: {}",
            err.message
        );
    }

    #[test]
    fn internal_without_falkor_error_has_no_hint() {
        let err = internal(anyhow::anyhow!("plain failure"));
        assert_eq!(&*err.message, "plain failure");
    }

    #[test]
    fn get_info_advertises_read_only_tools() {
        let info = server().get_info();
        assert!(info.capabilities.tools.is_some());
        assert!(info
            .instructions
            .expect("instructions")
            .contains("read-only"));
    }

    #[test]
    fn dtos_serialize_with_snake_case_fields() {
        let schema = Schema {
            labels: vec!["Person".into()],
            relationship_types: vec!["ACTED_IN".into()],
            property_keys: vec!["name".into()],
            indexes: vec![],
            constraints: vec![],
        };
        let json = serde_json::to_value(&schema).unwrap();
        assert!(json.get("relationship_types").is_some());
        assert!(json.get("property_keys").is_some());

        let out = QueryOutput {
            columns: vec!["n".into()],
            rows: vec![vec![serde_json::json!(1)]],
            truncated: true,
        };
        assert_eq!(
            serde_json::to_value(&out).unwrap()["truncated"],
            serde_json::json!(true)
        );
    }
}
