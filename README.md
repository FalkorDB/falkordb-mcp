# falkordb-mcp

> A [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server that lets AI assistants
> explore a **live** [FalkorDB](https://www.falkordb.com/) graph database — list graphs, inspect
> schema, run read-only Cypher, and view query plans.

`falkordb-mcp` speaks MCP over **stdio** (built on the [`rmcp`](https://crates.io/crates/rmcp) SDK)
and connects to FalkorDB with the async [`falkordb`](https://crates.io/crates/falkordb) client. It is
the companion server to the [`falkordb-rs`](https://github.com/FalkorDB/falkordb-rs) client library.

**Read-only by default.** The read tools run through the FalkorDB `GRAPH.RO_QUERY` command, which the
server rejects if a query attempts a write. **Guarded write tools are opt-in** — `query_write` and
`profile` are only exposed when the operator starts the server with `FALKORDB_MCP_ALLOW_WRITES=1`.

## Tools

| Tool | Input | Description |
| --- | --- | --- |
| `list_graphs` | — | List the names of all graphs on the FalkorDB server. |
| `get_schema` | `graph` | A graph's schema: labels, relationship types, property keys, indexes, constraints. |
| `query_read` | `graph`, `cypher`, `params?`, `limit?` | Run a **read-only** Cypher query and return rows as JSON. Writes are rejected server-side. |
| `explain` | `graph`, `cypher` | Return the query plan for a Cypher query **without executing it**. |
| `query_write` 🔒 | `graph`, `cypher`, `params?`, `limit?` | Run a **writing** Cypher query (`CREATE`/`MERGE`/`SET`/`DELETE`) and return rows. |
| `profile` 🔒 | `graph`, `cypher` | **Execute** a query and return its profiled plan (`GRAPH.PROFILE` runs it). |

🔒 = write-gated: present only when `FALKORDB_MCP_ALLOW_WRITES=1` (see [Writes](#writes-opt-in)).

Results are capped (`limit`, default `FALKORDB_MCP_MAX_ROWS`) so a broad query can't flood the model's
context; a capped result is flagged as truncated.

## Why an MCP server (not a Markdown doc or a "skill")?

You could instead describe your graph to the assistant some other way — a Markdown file, an `llms.txt`,
a packaged "skill", or a retrieval index over a database dump. All of those inject **static text** into
the model's context. An MCP server gives it **live, governed access to the running database**. That
difference is the whole point:

**1. Live truth, not a stale snapshot.** A doc describes the schema as it was the day someone wrote it.
Rename a label, add an index, or load new data and the doc lies — silently. `get_schema` reports what
the database actually contains right now.

```text
Doc/skill says:  (:Person)-[:ACTED_IN]->(:Movie)
Reality:         last week :Movie was renamed :Film
get_schema:      returns ["Person", "Film"]   ← the model uses the real name
```

**2. Grounded answers, not guesses.** With a doc or skill the model *writes a query it hopes is right*
and hands you prose. With MCP it runs the query and answers from real rows.

```text
You:    "How many films has Keanu Reeves acted in?"
Skill:  "Try: MATCH (p:Person {name:'Keanu Reeves'})-[:ACTED_IN]->(f:Film) RETURN count(f)"
MCP:    calls query_read → 7              ← an actual answer, not homework
```

**3. A feedback loop, not a one-shot dump.** MCP is a protocol, so the model can *chain* calls: read the
schema, `explain` a query to check its plan, then `query_read`. Each result informs the next call. A
Markdown blob is swallowed (or ignored) all at once, with no way to react to what's really there.

**4. Safety the model can't opt out of.** A skill that says "only read, never write" is a *polite
request* the model can disregard. `query_read` runs through the FalkorDB `GRAPH.RO_QUERY` command, so
the **server** rejects writes; results are capped; and connection credentials never enter the model's
context at all.

```text
Model emits:  MATCH (n) DETACH DELETE n
Skill:        relies on the model choosing to behave
MCP:          server rejects it (read-only) — the guarantee is structural, not advisory
```

**5. Write once, run in any assistant.** The same server works in Claude Desktop, Cursor, Zed, Cline and
any other MCP client. A skill or prompt is bespoke to one assistant and has to be re-pasted and
re-maintained everywhere.

**6. Only the context you need.** Pasting a whole schema plus sample rows into every prompt burns tokens
and still drifts out of date. MCP fetches just the labels, plan, or rows a given question needs, on
demand.

In short: Markdown and skills *describe* your database; an MCP server *connects the model to it* —
current, grounded, bounded, and portable. (Static docs still shine for things that rarely change, like
this client's API — that's what an [`llms.txt`](https://github.com/FalkorDB/falkordb-rs) is for. Use
each where it fits: docs for the stable shape, MCP for the live data.)

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
| `FALKORDB_MCP_MAX_ROWS` | `1000` | Default row cap for `query_read`/`query_write`. |
| `FALKORDB_MCP_ALLOW_WRITES` | `0` | Set to `1`/`true`/`yes`/`on` to expose the write-gated tools. |

All logging goes to **stderr** (`RUST_LOG` controls verbosity); **stdout is reserved for the MCP
protocol**.

## Writes (opt-in)

Writes are **off by default**. Start the server with `FALKORDB_MCP_ALLOW_WRITES=1` to expose two extra
tools:

- `query_write` — runs a writing Cypher query (`GRAPH.QUERY`).
- `profile` — **executes** a query and returns its profiled plan (`GRAPH.PROFILE` runs the query, so
  it is classified write-capable).

When writes are disabled, these tools are not even listed in `tools/list`, so the assistant can't see
or call them; a call is also rejected server-side as a second line of defense. Even with writes
enabled, your MCP client still asks you to approve each tool call — the env flag is the operator's
opt-in, the per-call prompt is yours.

```json
{
  "mcpServers": {
    "falkordb": {
      "command": "falkordb-mcp",
      "env": {
        "FALKORDB_URL": "falkor://127.0.0.1:6379",
        "FALKORDB_MCP_ALLOW_WRITES": "1"
      }
    }
  }
}
```

## Safety

- **Read-only by construction.** `query_read` uses `GRAPH.RO_QUERY`; the server rejects writes — the
  server never parses Cypher to guess intent. `explain` does not execute the query.
- **Writes are opt-in and gated.** `query_write`/`profile` are only registered when
  `FALKORDB_MCP_ALLOW_WRITES=1`, and re-checked at call time.
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

The required test gate (`just test`) is **hermetic**: tools are tested through the `FalkorBackend`
trait with a fake implementation, so no database is required. The live `#[ignore]`d tests are opt-in
via `just test-integration` (and the `db-*` Docker helpers manage a server) — they are never part of
the required gate, but they do run, against a FalkorDB service container, in the `coverage` job, so
the real backend is covered too. Use `just coverage-local` to collect coverage locally with Docker.

See [`.github/copilot-instructions.md`](.github/copilot-instructions.md) for the full contribution
conventions.

## License

[MIT](LICENSE) © FalkorDB
