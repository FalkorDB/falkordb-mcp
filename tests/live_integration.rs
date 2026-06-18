//! Opt-in live integration tests against a real FalkorDB server.
//!
//! These are `#[ignore]`d so they never run in the hermetic CI suite. Run them with a server up:
//!
//! ```text
//! just test-integration            # against an already-running server (FALKORDB_URL)
//! just test-integration-local      # spin up FalkorDB in Docker, run these, tear it down
//! ```
//!
//! Each test seeds its own uniquely named graph with the raw `falkordb` client (the MCP backend is
//! read-only), exercises [`FalkorClientBackend`], then deletes the graph. All fallible work is
//! captured into a `Result` and the graph is dropped *before* anything is asserted, so a failure
//! never leaks a graph on a shared server.

use std::collections::BTreeMap;
use std::future::Future;

use falkordb::{FalkorClientBuilder, FalkorConnectionInfo};
use falkordb_mcp::backend::FalkorBackend;
use falkordb_mcp::falkor::FalkorClientBackend;
use serde_json::json;

fn url() -> String {
    std::env::var("FALKORDB_URL").unwrap_or_else(|_| "falkor://127.0.0.1:6379".into())
}

/// Create a uniquely named graph, seed it with `setup_cypher`, and return its name.
async fn seed_graph(
    suffix: &str,
    setup_cypher: &str,
) -> String {
    let name = format!("falkordb_mcp_it_{}_{}", std::process::id(), suffix);
    let info: FalkorConnectionInfo = url().as_str().try_into().expect("connection info");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("seed client connects");
    let mut graph = client.select_graph(&name);
    graph.delete().await.ok(); // ignore "graph does not exist"
    graph
        .query(setup_cypher)
        .execute()
        .await
        .expect("seed query runs");
    name
}

async fn drop_graph(name: &str) {
    let info: FalkorConnectionInfo = url().as_str().try_into().expect("connection info");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("cleanup client connects");
    client.select_graph(name).delete().await.ok();
}

/// Seed a graph, connect a backend, run `op`, then **always** drop the graph. Returns `op`'s result
/// (a connect failure surfaces as `Err`) together with the graph name, so the caller asserts only
/// after cleanup has run.
async fn run_live<F, Fut, T>(
    suffix: &str,
    setup_cypher: &str,
    op: F,
) -> (anyhow::Result<T>, String)
where
    F: FnOnce(FalkorClientBackend, String) -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let name = seed_graph(suffix, setup_cypher).await;
    let result = async {
        let backend = FalkorClientBackend::connect(&url()).await?;
        op(backend, name.clone()).await
    }
    .await;
    drop_graph(&name).await;
    (result, name)
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn list_graphs_includes_seeded_graph() {
    let (result, name) = run_live(
        "list",
        "CREATE (:Person {name: 'Neo'})",
        |backend, _name| async move { backend.list_graphs().await },
    )
    .await;

    let graphs = result.expect("list_graphs");
    assert!(graphs.contains(&name), "expected {name} in {graphs:?}");
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn schema_reports_labels_types_and_keys() {
    let (result, _name) = run_live(
        "schema",
        "CREATE (:Person {name: 'Neo'})-[:KNOWS {since: 1999}]->(:Person {name: 'Trinity'})",
        |backend, name| async move { backend.schema(&name).await },
    )
    .await;

    let schema = result.expect("schema");
    assert!(schema.labels.contains(&"Person".to_string()), "{schema:?}");
    assert!(
        schema.relationship_types.contains(&"KNOWS".to_string()),
        "{schema:?}"
    );
    assert!(
        schema.property_keys.contains(&"name".to_string()),
        "{schema:?}"
    );
    assert!(
        schema.property_keys.contains(&"since".to_string()),
        "{schema:?}"
    );
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_binds_params_and_returns_rows() {
    let (result, _name) = run_live(
        "read",
        "CREATE (:Person {name: 'Neo'}), (:Person {name: 'Trinity'})",
        |backend, name| async move {
            let mut params = BTreeMap::new();
            params.insert("name".to_string(), json!("Neo"));
            backend
                .read_query(
                    &name,
                    "MATCH (p:Person {name: $name}) RETURN p.name AS name",
                    params,
                    100,
                )
                .await
        },
    )
    .await;

    let out = result.expect("read_query");
    assert_eq!(out.columns, vec!["name".to_string()]);
    assert_eq!(out.rows, vec![vec![json!("Neo")]]);
    assert!(!out.truncated);
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_caps_rows_and_flags_truncation() {
    let (result, _name) = run_live(
        "cap",
        "UNWIND range(1, 5) AS i CREATE (:N {i: i})",
        |backend, name| async move {
            backend
                .read_query(&name, "MATCH (n:N) RETURN n.i AS i", BTreeMap::new(), 2)
                .await
        },
    )
    .await;

    let out = result.expect("read_query");
    assert_eq!(out.rows.len(), 2, "capped to the row limit");
    assert!(out.truncated, "more rows than the cap → truncated");
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_rejects_writes() {
    // The op returns `Ok(<the read_query result>)`, so a connect failure (outer `Err`) is
    // distinguished from the write being rejected (inner `Err`) — a connection problem can't
    // masquerade as proof that writes are blocked.
    let (result, _name) = run_live(
        "ro",
        "CREATE (:Person {name: 'Neo'})",
        |backend, name| async move {
            Ok(backend
                .read_query(&name, "CREATE (:Sneaky)", BTreeMap::new(), 100)
                .await)
        },
    )
    .await;

    let write_result = result.expect("connect and issue the query");
    assert!(write_result.is_err(), "writes must be rejected by ro_query");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn explain_returns_a_plan_without_executing() {
    let (result, _name) = run_live(
        "explain",
        "CREATE (:Person {name: 'Neo'})",
        |backend, name| async move { backend.explain(&name, "MATCH (p:Person) RETURN p").await },
    )
    .await;

    let plan = result.expect("explain");
    assert!(!plan.is_empty(), "plan should have at least one step");
}
