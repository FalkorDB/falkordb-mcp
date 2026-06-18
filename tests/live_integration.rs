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
//! read-only), exercises [`FalkorClientBackend`], then deletes the graph.

use std::collections::BTreeMap;

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

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn list_graphs_includes_seeded_graph() {
    let name = seed_graph("list", "CREATE (:Person {name: 'Neo'})").await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    let graphs = backend.list_graphs().await.expect("list_graphs");
    assert!(graphs.contains(&name), "expected {name} in {graphs:?}");

    drop_graph(&name).await;
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn schema_reports_labels_types_and_keys() {
    let name = seed_graph(
        "schema",
        "CREATE (:Person {name: 'Neo'})-[:KNOWS {since: 1999}]->(:Person {name: 'Trinity'})",
    )
    .await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    let schema = backend.schema(&name).await.expect("schema");
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

    drop_graph(&name).await;
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_binds_params_and_returns_rows() {
    let name = seed_graph(
        "read",
        "CREATE (:Person {name: 'Neo'}), (:Person {name: 'Trinity'})",
    )
    .await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    let mut params = BTreeMap::new();
    params.insert("name".to_string(), json!("Neo"));
    let out = backend
        .read_query(
            &name,
            "MATCH (p:Person {name: $name}) RETURN p.name AS name",
            params,
            100,
        )
        .await
        .expect("read_query");

    assert_eq!(out.columns, vec!["name".to_string()]);
    assert_eq!(out.rows, vec![vec![json!("Neo")]]);
    assert!(!out.truncated);

    drop_graph(&name).await;
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_caps_rows_and_flags_truncation() {
    let name = seed_graph("cap", "UNWIND range(1, 5) AS i CREATE (:N {i: i})").await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    let out = backend
        .read_query(&name, "MATCH (n:N) RETURN n.i AS i", BTreeMap::new(), 2)
        .await
        .expect("read_query");

    assert_eq!(out.rows.len(), 2, "capped to the row limit");
    assert!(out.truncated, "more rows than the cap → truncated");

    drop_graph(&name).await;
}

#[tokio::test]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn read_query_rejects_writes() {
    let name = seed_graph("ro", "CREATE (:Person {name: 'Neo'})").await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    // GRAPH.RO_QUERY must refuse a write — proving read-only safety is server-enforced.
    let result = backend
        .read_query(&name, "CREATE (:Sneaky)", BTreeMap::new(), 100)
        .await;
    assert!(result.is_err(), "writes must be rejected by ro_query");

    drop_graph(&name).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live FalkorDB server (just test-integration)"]
async fn explain_returns_a_plan_without_executing() {
    let name = seed_graph("explain", "CREATE (:Person {name: 'Neo'})").await;
    let backend = FalkorClientBackend::connect(&url()).await.expect("connect");

    let plan = backend
        .explain(&name, "MATCH (p:Person) RETURN p")
        .await
        .expect("explain");
    assert!(!plan.is_empty(), "plan should have at least one step");

    drop_graph(&name).await;
}
