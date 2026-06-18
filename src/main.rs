//! `falkordb-mcp` binary: connect to FalkorDB and serve the MCP tools over stdio.

use std::sync::Arc;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

use falkordb_mcp::{config::Config, falkor::FalkorClientBackend, server::FalkorMcp};

#[tokio::main]
async fn main() -> Result<()> {
    // The MCP protocol owns stdout; send all logs to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(url = %config.url, max_rows = config.max_rows, "starting falkordb-mcp");

    let backend = FalkorClientBackend::connect(&config.url).await?;
    let server = FalkorMcp::new(Arc::new(backend), config.max_rows);

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
