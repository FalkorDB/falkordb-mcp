# Upstream notes for `falkordb-rs`

Changes to the [`falkordb`](https://github.com/FalkorDB/falkordb-rs) client crate that surfaced while
building this MCP server. **None of these block shipping** — `falkordb-mcp` works today with the
workarounds noted below — but each would let us delete a workaround or simplify the code. They are
ordered roughly by impact.

The behavior below was observed against `falkordb = "0.8.6"`.

## 1. `QueryBuilder<Output>` is `!Send` when `Output: !Send` (blocks async `explain`)

**What.** `QueryBuilder` holds `_unused: PhantomData<Output>`. For `explain()`, `Output` is
`ExecutionPlan`, which is `!Send` (it stores `Rc<Operation>`). `PhantomData<ExecutionPlan>` is therefore
`!Send`, which makes the whole `QueryBuilder<ExecutionPlan, …>` — and the `execute()` future that owns
it — `!Send`.

**Why it matters for the MCP server.** The tool layer calls the backend through an
`#[async_trait]` trait whose futures must be `Send` (rmcp serves on a multi-threaded runtime). A plain
`graph.explain(cypher).execute().await` inside that trait method fails to compile with
`future cannot be sent between threads safely`.

**Current workaround** (`src/falkor.rs`, `explain`). Drive the `!Send` future to completion
synchronously with `tokio::task::block_in_place` + `Handle::block_on`, so the trait method body has no
`await` and stays `Send`. This requires the multi-threaded runtime (the binary uses `#[tokio::main]`)
and adds a worker hand-off per `explain` call.

**Proposed upstream change.** Either (preferred, one line) make the marker variance-only so a `!Send`
`Output` can't infect the builder:

```rust
// QueryBuilder
_unused: PhantomData<fn() -> Output>,   // instead of PhantomData<Output>
```

and/or make `ExecutionPlan` itself `Send` by storing `Arc<Operation>` instead of `Rc<Operation>`.
Either change lets `explain` be a normal `.await` on any runtime and removes the workaround.

## 2. `Vec32` is not re-exported at the crate root

**What.** `FalkorValue::Vec32(Vec32)` is a public variant, but the `Vec32` type is not re-exported from
`falkordb` (only reachable as the private `value::vec32::Vec32`). External code can *match* the variant
but cannot *name* the type to construct it.

**Why it matters.** The DTO conversion can read `FalkorValue::Vec32(v)` fine, but the unit test for that
conversion arm can't build a `Vec32` value, so that one branch is left to the live integration tests.

**Current workaround.** Skip constructing `Vec32` in the hermetic conversion tests.

**Proposed upstream change.** `pub use value::vec32::Vec32;` (and audit the other `FalkorValue` payload
types — `Point`, `Path`, `Node`, `Edge` are already re-exported; `Vec32` is the gap).

## 3. No `serde::Serialize` for `FalkorValue` and the graph entities

**What.** `FalkorValue`, `Node`, `Edge`, `Path`, `Point`, `Vec32` derive `Deserialize` paths but not
`Serialize`, so there is no built-in way to turn a query result into JSON.

**Why it matters.** The server must render results as JSON for the model, so it carries an explicit
`value_to_json` mapping (`src/falkor.rs`).

**Current workaround.** The bespoke DTO mapping — which is fine, and arguably desirable since the MCP
server wants to pin its own stable JSON shape regardless of the client's internals.

**Proposed upstream change** (optional). A feature-gated `Serialize` implementation, or a documented canonical
JSON representation, would let simpler consumers skip writing their own. Keep it behind a feature so it
doesn't force a wire format on everyone.

## 4. `RowStream` is materialized eagerly

**What.** `RowStream` wraps a `VecDeque<FalkorResult<Row>>` — all rows are parsed into memory before the
stream is handed back; it is a buffer, not a lazy cursor.

**Why it matters.** The server caps how many rows it forwards to the model, but the cap only limits what
is *serialized*, not what the server computes or buffers. A pathological query still materializes its
full result set first.

**Current workaround.** Bound server work with `with_timeout(…)`, cap the output rows, and flag
`truncated` when the cap is hit.

**Proposed upstream change** (nice-to-have). A genuinely lazy/streaming `RowStream`, or a builder
`.with_row_limit(n)` that pushes the cap server-side, so large results don't have to be buffered.

## 5. Read-only schema introspection helper

**What.** Building a graph's schema requires several calls: `CALL db.labels()`,
`CALL db.relationshipTypes()`, `CALL db.propertyKeys()`, plus `list_indices()` and `list_constraints()`,
each parsed and stitched together by hand.

**Why it matters.** Every consumer that wants "the shape of this graph" repeats the same stitching
(see `schema` in `src/falkor.rs`).

**Proposed upstream change** (nice-to-have). A `graph.schema()` (or a grouped read-only introspection
helper) returning labels, relationship types, property keys, indexes and constraints in one call.

## 6. Credential-safe `FalkorConnectionInfo` errors / `Display`

**What.** A connection URL can embed credentials (`falkor://user:pass@host`). Parse/connect errors and
any `Display` of the connection info risk echoing them.

**Why it matters.** A server should never leak credentials into logs or into errors returned to a
client.

**Current workaround.** `connect()` never echoes the URL; parse failures return a generic
"check the `FALKORDB_URL` environment variable" message.

**Proposed upstream change** (nice-to-have). A redacted `Display`/`Debug` for `FalkorConnectionInfo`
(masking any user/password) and error messages that never include the raw URL.

---

When an upstream change lands, remove the corresponding workaround here and update the reference in the
code comment (currently only `src/falkor.rs::explain` points back to this file).
