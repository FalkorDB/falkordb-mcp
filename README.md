# falkordb-mcp

> A [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server that lets AI assistants
> explore a **live** [FalkorDB](https://www.falkordb.com/) graph database — list graphs, inspect
> schema, run read-only Cypher, and view query plans.

`falkordb-mcp` speaks MCP over **stdio** (built on the [`rmcp`](https://crates.io/crates/rmcp) SDK)
and connects to FalkorDB with the async [`falkordb`](https://crates.io/crates/falkordb) client. It is
the companion server to the [`falkordb-rs`](https://github.com/FalkorDB/falkordb-rs) client library.

**v1 is read-only by design.** Every query runs through the FalkorDB `GRAPH.RO_QUERY` command, which
the server rejects if it attempts a write. Guarded write tools are a later, opt-in addition.

## Tools

| Tool | Input | Description |
| --- | --- | --- |
| `list_graphs` | — | List the names of all graphs on the FalkorDB server. |
| `get_schema` | `graph` | A graph's schema: labels, relationship types, property keys, indexes, constraints. |
| `query_read` | `graph`, `cypher`, `params?`, `limit?` | Run a **read-only** Cypher query and return rows as JSON. Writes are rejected server-side. |
| `explain` | `graph`, `cypher` | Return the query plan for a Cypher query **without executing it**. |

Results are capped (`limit`, default `FALKORDB_MCP_MAX_ROWS`) so a broad query can't flood the model's
context; a capped result is flagged as truncated.

## Install

From source:

```bash
git clone https://github.com/FalkorDB/falkordb-mcp
cd falkordb-mcp
cargo build --release
# binary at target/release/falkordb-mcp
```

Or with cargo once published:

```bash
cargo install falkordb-mcp
```

## Configure your MCP client

`falkordb-mcp` is a stdio server, so point your assistant at the binary. For example, in a Claude
Desktop / Cursor `mcpServers` block:

```json
{
  "mcpServers": {
    "falkordb": {
      "command": "falkordb-mcp",
      "env": {
        "FALKORDB_URL": "falkor://127.0.0.1:6379"
      }
    }
  }
}
```

(Use an absolute path to the binary if it isn't on your `PATH`.)

## Configuration

The connection is taken from the **operator's environment only**, never from a tool call:

| Variable | Default | Purpose |
| --- | --- | --- |
| `FALKORDB_URL` | `falkor://127.0.0.1:6379` | FalkorDB connection URL. |
| `FALKORDB_MCP_MAX_ROWS` | `1000` | Default row cap for `query_read`. |

All logging goes to **stderr** (`RUST_LOG` controls verbosity); **stdout is reserved for the MCP
protocol**.

## Safety

- **Read-only by construction.** `query_read` uses `GRAPH.RO_QUERY`; the server rejects writes — the
  server never parses Cypher to guess intent. `explain` does not execute the query.
- **No credentials in tool surface.** Connection details come only from the environment, and
  credentials are scrubbed from any error returned to the client.
- **Bounded output.** Row caps keep results from overwhelming the model.

## Development

This repo is `just`-driven; every CI check has a matching recipe. Run `just --list` to see them all.

```bash
just check      # fast loop: fmt + clippy + build
just done       # definition-of-done gates (fmt, clippy, clippy-all, build, doc, deny, test)
just verify     # all CI gates + coverage
just test       # hermetic test suite (fake backend — no FalkorDB server needed)
just spellcheck # spellcheck the Markdown docs
```

The unit/integration suite is **hermetic**: tools are tested through the `FalkorBackend` trait with a
fake implementation, so no database is required. The opt-in `just test-integration` recipe (and the
`db-*` Docker helpers) exercise a real server and are never a CI gate.

See [`.github/copilot-instructions.md`](.github/copilot-instructions.md) for the full contribution
conventions.

## License

[MIT](LICENSE) © FalkorDB
