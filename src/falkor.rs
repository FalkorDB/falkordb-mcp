//! The real backend: wraps the async `falkordb` client. (Implementation lands next; this compiles so
//! the rmcp wiring can be validated first.)

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::backend::{FalkorBackend, QueryOutput, Schema};

/// Read-only FalkorDB access over the async client.
pub struct FalkorClientBackend {}

impl FalkorClientBackend {
    /// Connect to FalkorDB at `url` (e.g. `falkor://127.0.0.1:6379`).
    pub async fn connect(_url: &str) -> anyhow::Result<Self> {
        anyhow::bail!("FalkorClientBackend::connect not yet implemented")
    }
}

#[async_trait]
impl FalkorBackend for FalkorClientBackend {
    async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
        anyhow::bail!("not implemented")
    }

    async fn schema(
        &self,
        _graph: &str,
    ) -> anyhow::Result<Schema> {
        anyhow::bail!("not implemented")
    }

    async fn read_query(
        &self,
        _graph: &str,
        _cypher: &str,
        _params: BTreeMap<String, serde_json::Value>,
        _limit: usize,
    ) -> anyhow::Result<QueryOutput> {
        anyhow::bail!("not implemented")
    }

    async fn explain(
        &self,
        _graph: &str,
        _cypher: &str,
    ) -> anyhow::Result<Vec<String>> {
        anyhow::bail!("not implemented")
    }
}
