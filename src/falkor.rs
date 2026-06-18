//! The real backend: wraps the async `falkordb` client.
//!
//! All graph access is **read-only** (`GRAPH.RO_QUERY` / `GRAPH.EXPLAIN`). `FalkorValue` and the graph
//! entity types do not derive `Serialize`, so results are mapped to JSON through the explicit DTO
//! helpers below ([`value_to_json`] and friends), which are pure and unit-tested without a database.

use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use falkordb::{
    AsyncGraph, Edge, FalkorAsyncClient, FalkorClientBuilder, FalkorConnectionInfo, FalkorValue,
    Node, Path, Point, Row,
};
use serde_json::{Map, Value};
use tokio_stream::StreamExt;

use crate::backend::{FalkorBackend, QueryOutput, Schema};

/// Server-side query timeout (ms). `RowStream` is materialized eagerly, so this — not the output row
/// cap — is what bounds work for a runaway query; the server aborts and returns a timeout error.
const QUERY_TIMEOUT_MS: i64 = 10_000;

/// Read-only FalkorDB access over the async client.
pub struct FalkorClientBackend {
    client: FalkorAsyncClient,
}

impl FalkorClientBackend {
    /// Connect to FalkorDB at `url` (e.g. `falkor://127.0.0.1:6379`).
    ///
    /// Errors here are operator-facing (startup, logged to stderr) and never echo the URL, which may
    /// carry credentials.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let connection_info: FalkorConnectionInfo = url.try_into().map_err(|_| {
            anyhow::anyhow!("invalid FALKORDB_URL (check the FALKORDB_URL environment variable)")
        })?;
        let client = FalkorClientBuilder::new_async()
            .with_connection_info(connection_info)
            .build()
            .await
            .map_err(|_| {
                // Deliberately not interpolating the client error: it may carry the connection
                // string (and thus credentials). Keep operator logs credential-free.
                anyhow::anyhow!(
                    "failed to connect to FalkorDB (check FALKORDB_URL and that the server is reachable)"
                )
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl FalkorBackend for FalkorClientBackend {
    async fn list_graphs(&self) -> anyhow::Result<Vec<String>> {
        Ok(self.client.list_graphs().await?)
    }

    async fn schema(
        &self,
        graph: &str,
    ) -> anyhow::Result<Schema> {
        let mut graph = self.client.select_graph(graph);
        let labels = single_column(&mut graph, "CALL db.labels()").await?;
        let relationship_types = single_column(&mut graph, "CALL db.relationshipTypes()").await?;
        let property_keys = single_column(&mut graph, "CALL db.propertyKeys()").await?;

        let indexes = graph
            .list_indices()
            .await?
            .data
            .into_iter()
            .map(|i| {
                format!(
                    "{} :{}({})",
                    i.entity_type,
                    i.index_label,
                    i.fields.join(", ")
                )
            })
            .collect();
        let constraints = graph
            .list_constraints()
            .await?
            .data
            .into_iter()
            .map(|c| {
                format!(
                    "{} {} :{}({})",
                    c.constraint_type,
                    c.entity_type,
                    c.label,
                    c.properties.join(", ")
                )
            })
            .collect();

        Ok(Schema {
            labels,
            relationship_types,
            property_keys,
            indexes,
            constraints,
        })
    }

    async fn read_query(
        &self,
        graph: &str,
        cypher: &str,
        params: BTreeMap<String, serde_json::Value>,
        limit: usize,
    ) -> anyhow::Result<QueryOutput> {
        let falkor_params: BTreeMap<String, FalkorValue> = params
            .into_iter()
            .map(|(k, v)| (k, json_to_falkor(v)))
            .collect();

        let mut graph = self.client.select_graph(graph);
        let result = graph
            .ro_query(cypher)
            .with_params(falkor_params)
            .with_timeout(QUERY_TIMEOUT_MS)
            .execute()
            .await?;

        let columns: Vec<String> = result.header.iter().cloned().collect();
        let mut data = result.data;
        let total = data.len();
        let truncated = total > limit;

        let mut rows = Vec::with_capacity(total.min(limit));
        while rows.len() < limit {
            match data.next().await {
                Some(row) => rows.push(row_to_json(row?)),
                None => break,
            }
        }

        Ok(QueryOutput {
            columns,
            rows,
            truncated,
        })
    }

    async fn explain(
        &self,
        graph: &str,
        cypher: &str,
    ) -> anyhow::Result<Vec<String>> {
        // `falkordb`'s `ExecutionPlan` holds `Rc`, so `explain().execute()` is a `!Send` future that
        // can't be `.await`ed directly inside this `Send`-required trait method. Drive it to
        // completion on the current worker via `block_in_place` + `Handle::block_on` (there is no
        // `.await` in this method body, so its own future stays `Send`). This needs the multi-thread
        // runtime, which the binary uses (`#[tokio::main]`). See `docs/upstream-falkordb-rs.md` for
        // the one-line upstream change that would let this be a plain `.await` on any runtime.
        if tokio::runtime::Handle::current().runtime_flavor()
            == tokio::runtime::RuntimeFlavor::CurrentThread
        {
            anyhow::bail!(
                "the explain tool requires the server to run on a multi-threaded runtime"
            );
        }
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut graph = self.client.select_graph(graph);
                let plan = graph.explain(cypher).execute().await?;
                anyhow::Ok(plan.plan().to_vec())
            })
        })
    }
}

/// Run a read-only query that yields a single column, returning that column's values as strings.
async fn single_column(
    graph: &mut AsyncGraph,
    query: &str,
) -> anyhow::Result<Vec<String>> {
    let mut data = graph
        .ro_query(query)
        .with_timeout(QUERY_TIMEOUT_MS)
        .execute()
        .await?
        .data;
    let mut out = Vec::with_capacity(data.len());
    while let Some(row) = data.next().await {
        if let Some(value) = row?.into_values().into_iter().next() {
            out.push(falkor_scalar_to_string(value));
        }
    }
    Ok(out)
}

/// A scalar (label/type/key) as a plain string — unquoted when it is already a string.
fn falkor_scalar_to_string(value: FalkorValue) -> String {
    match value_to_json(value) {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

/// Convert a row into a JSON array of its values, in column order.
fn row_to_json(row: Row) -> Vec<Value> {
    row.into_values().into_iter().map(value_to_json).collect()
}

/// Map a [`FalkorValue`] to JSON via explicit DTOs (the FalkorDB types do not derive `Serialize`).
fn value_to_json(value: FalkorValue) -> Value {
    match value {
        FalkorValue::None => Value::Null,
        FalkorValue::Bool(b) => Value::Bool(b),
        FalkorValue::I64(i) => Value::from(i),
        FalkorValue::F64(f) => json_number(f),
        FalkorValue::String(s) => Value::String(s),
        FalkorValue::Array(items) => Value::Array(items.into_iter().map(value_to_json).collect()),
        FalkorValue::Map(map) => Value::Object(map_to_json(map)),
        FalkorValue::Node(node) => node_to_json(node),
        FalkorValue::Edge(edge) => edge_to_json(edge),
        FalkorValue::Path(path) => path_to_json(path),
        FalkorValue::Point(point) => point_to_json(&point),
        FalkorValue::Vec32(vec) => Value::Array(
            vec.values
                .into_iter()
                .map(|f| json_number(f as f64))
                .collect(),
        ),
        FalkorValue::Unparseable(raw) => {
            let mut obj = Map::new();
            obj.insert("unparseable".into(), Value::String(raw));
            Value::Object(obj)
        }
    }
}

/// Convert an incoming JSON parameter into a [`FalkorValue`] for safe, named query binding.
fn json_to_falkor(value: Value) -> FalkorValue {
    match value {
        Value::Null => FalkorValue::None,
        Value::Bool(b) => FalkorValue::Bool(b),
        Value::Number(n) => n
            .as_i64()
            .map(FalkorValue::I64)
            .or_else(|| n.as_f64().map(FalkorValue::F64))
            .unwrap_or(FalkorValue::None),
        Value::String(s) => FalkorValue::String(s),
        Value::Array(items) => FalkorValue::Array(items.into_iter().map(json_to_falkor).collect()),
        Value::Object(obj) => FalkorValue::Map(
            obj.into_iter()
                .map(|(k, v)| (k, json_to_falkor(v)))
                .collect(),
        ),
    }
}

fn map_to_json(map: HashMap<String, FalkorValue>) -> Map<String, Value> {
    map.into_iter()
        .map(|(k, v)| (k, value_to_json(v)))
        .collect()
}

fn node_to_json(node: Node) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), Value::from(node.entity_id));
    obj.insert("labels".into(), Value::from(node.labels));
    obj.insert(
        "properties".into(),
        Value::Object(map_to_json(node.properties)),
    );
    Value::Object(obj)
}

fn edge_to_json(edge: Edge) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), Value::from(edge.entity_id));
    obj.insert("type".into(), Value::String(edge.relationship_type));
    obj.insert("start_node".into(), Value::from(edge.src_node_id));
    obj.insert("end_node".into(), Value::from(edge.dst_node_id));
    obj.insert(
        "properties".into(),
        Value::Object(map_to_json(edge.properties)),
    );
    Value::Object(obj)
}

fn path_to_json(path: Path) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "nodes".into(),
        Value::Array(path.nodes.into_iter().map(node_to_json).collect()),
    );
    obj.insert(
        "relationships".into(),
        Value::Array(path.relationships.into_iter().map(edge_to_json).collect()),
    );
    Value::Object(obj)
}

fn point_to_json(point: &Point) -> Value {
    let mut obj = Map::new();
    obj.insert("latitude".into(), json_number(point.latitude));
    obj.insert("longitude".into(), json_number(point.longitude));
    Value::Object(obj)
}

/// A finite `f64` as a JSON number; non-finite values (`NaN`/`±inf`, which JSON can't represent)
/// become `null`.
fn json_number(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(pairs: &[(&str, FalkorValue)]) -> HashMap<String, FalkorValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn scalars_map_to_json() {
        assert_eq!(value_to_json(FalkorValue::None), Value::Null);
        assert_eq!(value_to_json(FalkorValue::Bool(true)), json!(true));
        assert_eq!(value_to_json(FalkorValue::I64(42)), json!(42));
        assert_eq!(value_to_json(FalkorValue::F64(1.5)), json!(1.5));
        assert_eq!(value_to_json(FalkorValue::String("hi".into())), json!("hi"));
    }

    #[test]
    fn non_finite_float_becomes_null() {
        assert_eq!(value_to_json(FalkorValue::F64(f64::NAN)), Value::Null);
        assert_eq!(value_to_json(FalkorValue::F64(f64::INFINITY)), Value::Null);
    }

    #[test]
    fn array_and_map_recurse() {
        let array = FalkorValue::Array(vec![FalkorValue::I64(1), FalkorValue::String("x".into())]);
        assert_eq!(value_to_json(array), json!([1, "x"]));

        let map = FalkorValue::Map(props(&[("k", FalkorValue::Bool(false))]));
        assert_eq!(value_to_json(map), json!({ "k": false }));
    }

    #[test]
    fn node_maps_to_dto() {
        let node = FalkorValue::Node(Node {
            entity_id: 7,
            labels: vec!["Person".into(), "Actor".into()],
            properties: props(&[("name", FalkorValue::String("Neo".into()))]),
        });
        assert_eq!(
            value_to_json(node),
            json!({
                "id": 7,
                "labels": ["Person", "Actor"],
                "properties": { "name": "Neo" }
            })
        );
    }

    #[test]
    fn edge_maps_to_dto() {
        let edge = FalkorValue::Edge(Edge {
            entity_id: 3,
            relationship_type: "ACTED_IN".into(),
            src_node_id: 1,
            dst_node_id: 2,
            properties: props(&[("role", FalkorValue::String("Neo".into()))]),
        });
        assert_eq!(
            value_to_json(edge),
            json!({
                "id": 3,
                "type": "ACTED_IN",
                "start_node": 1,
                "end_node": 2,
                "properties": { "role": "Neo" }
            })
        );
    }

    #[test]
    fn path_and_point_map_to_dto() {
        let path = FalkorValue::Path(Path {
            nodes: vec![Node {
                entity_id: 1,
                labels: vec!["A".into()],
                properties: HashMap::new(),
            }],
            relationships: vec![],
        });
        assert_eq!(
            value_to_json(path),
            json!({
                "nodes": [{ "id": 1, "labels": ["A"], "properties": {} }],
                "relationships": []
            })
        );

        let point = FalkorValue::Point(Point {
            latitude: 1.0,
            longitude: 2.0,
        });
        assert_eq!(
            value_to_json(point),
            json!({ "latitude": 1.0, "longitude": 2.0 })
        );
    }

    #[test]
    fn unparseable_is_surfaced_not_dropped() {
        let value = FalkorValue::Unparseable("bad".into());
        assert_eq!(value_to_json(value), json!({ "unparseable": "bad" }));
    }

    #[test]
    fn json_params_map_to_falkor_values() {
        assert_eq!(json_to_falkor(json!(null)), FalkorValue::None);
        assert_eq!(json_to_falkor(json!(true)), FalkorValue::Bool(true));
        assert_eq!(json_to_falkor(json!(10)), FalkorValue::I64(10));
        assert_eq!(json_to_falkor(json!(2.5)), FalkorValue::F64(2.5));
        assert_eq!(json_to_falkor(json!("s")), FalkorValue::String("s".into()));
        assert_eq!(
            json_to_falkor(json!([1, 2])),
            FalkorValue::Array(vec![FalkorValue::I64(1), FalkorValue::I64(2)])
        );
        assert_eq!(
            json_to_falkor(json!({ "a": 1 })),
            FalkorValue::Map(props(&[("a", FalkorValue::I64(1))]))
        );
    }

    #[test]
    fn scalar_round_trips_json_to_falkor_to_json() {
        for original in [
            json!(null),
            json!(true),
            json!(7),
            json!("text"),
            json!([1, "a"]),
        ] {
            let round_tripped = value_to_json(json_to_falkor(original.clone()));
            assert_eq!(round_tripped, original);
        }
    }

    #[test]
    fn falkor_scalar_to_string_unquotes_strings() {
        assert_eq!(
            falkor_scalar_to_string(FalkorValue::String("Person".into())),
            "Person"
        );
        assert_eq!(falkor_scalar_to_string(FalkorValue::I64(5)), "5");
    }

    #[test]
    fn row_to_json_preserves_column_order() {
        let row = Row::from_iter([
            ("a".to_string(), FalkorValue::I64(1)),
            ("b".to_string(), FalkorValue::String("x".into())),
        ]);
        assert_eq!(row_to_json(row), vec![json!(1), json!("x")]);
    }
}
