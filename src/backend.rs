//! The backend abstraction the MCP tools call, plus the LLM-facing DTOs.
//!
//! Abstracting FalkorDB access behind a trait keeps the tool layer **hermetically testable** with a
//! fake backend (no database needed), while the real implementation ([`crate::falkor`]) wraps the
//! async `falkordb` client.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Serialize;

/// A read-only view of a graph's schema, for query authoring/planning.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Schema {
    pub labels: Vec<String>,
    pub relationship_types: Vec<String>,
    pub property_keys: Vec<String>,
    pub indexes: Vec<String>,
    pub constraints: Vec<String>,
}

/// The result of a read query, capped and shaped for an LLM.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct QueryOutput {
    /// Column names, in order.
    pub columns: Vec<String>,
    /// Rows; each value is a JSON rendering of a `FalkorValue` (see `crate::falkor::value_to_json`).
    pub rows: Vec<Vec<serde_json::Value>>,
    /// `true` if the result was truncated to the row cap.
    pub truncated: bool,
}

/// Read-only operations the MCP server exposes. Implementors must be cheap to share (`Arc`).
#[async_trait]
pub trait FalkorBackend: Send + Sync {
    /// Names of the graphs on the server.
    async fn list_graphs(&self) -> anyhow::Result<Vec<String>>;

    /// Labels, relationship types, property keys, indexes and constraints of `graph`.
    async fn schema(
        &self,
        graph: &str,
    ) -> anyhow::Result<Schema>;

    /// Run a **read-only** query (`GRAPH.RO_QUERY`, server-enforced) with a row cap and timeout.
    async fn read_query(
        &self,
        graph: &str,
        cypher: &str,
        params: BTreeMap<String, serde_json::Value>,
        limit: usize,
    ) -> anyhow::Result<QueryOutput>;

    /// The query plan for `cypher` without executing it (`GRAPH.EXPLAIN`).
    async fn explain(
        &self,
        graph: &str,
        cypher: &str,
    ) -> anyhow::Result<Vec<String>>;

    /// Run a **write** query (`GRAPH.QUERY`) with a row cap and timeout. Guarded: the server only
    /// exposes this when started with writes enabled.
    async fn write_query(
        &self,
        graph: &str,
        cypher: &str,
        params: BTreeMap<String, serde_json::Value>,
        limit: usize,
    ) -> anyhow::Result<QueryOutput>;

    /// Execute `cypher` and return its profiled plan (`GRAPH.PROFILE`). Guarded like
    /// [`write_query`](Self::write_query) because `PROFILE` **runs** the query.
    async fn profile(
        &self,
        graph: &str,
        cypher: &str,
    ) -> anyhow::Result<Vec<String>>;
}

/// A canned backend for tests and protocol smoke-checks — no database required.
#[cfg(test)]
pub struct FakeBackend;

#[cfg(test)]
#[async_trait]
impl FalkorBackend for FakeBackend {
    async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec!["social".into(), "imdb".into()])
    }

    async fn schema(
        &self,
        _graph: &str,
    ) -> anyhow::Result<Schema> {
        Ok(Schema {
            labels: vec!["Person".into(), "Film".into()],
            relationship_types: vec!["ACTED_IN".into()],
            property_keys: vec!["name".into(), "title".into()],
            indexes: vec!["Person(name)".into()],
            constraints: vec![],
        })
    }

    async fn read_query(
        &self,
        _graph: &str,
        _cypher: &str,
        _params: BTreeMap<String, serde_json::Value>,
        _limit: usize,
    ) -> anyhow::Result<QueryOutput> {
        Ok(QueryOutput {
            columns: vec!["name".into()],
            rows: vec![vec![serde_json::json!("Keanu")]],
            truncated: false,
        })
    }

    async fn explain(
        &self,
        _graph: &str,
        _cypher: &str,
    ) -> anyhow::Result<Vec<String>> {
        Ok(vec!["Results".into(), "    Project".into()])
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
        Ok(vec![
            "Results".into(),
            "    Create | Records created: 1".into(),
        ])
    }
}
