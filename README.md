[![Release](https://img.shields.io/github/release/falkordb/falkordb-mcp.svg)](https://github.com/falkordb/falkordb-mcp/releases/latest)
[![crates.io](https://img.shields.io/crates/dr/falkordb-mcp)](https://crates.io/crates/falkordb-mcp)
[![license](https://img.shields.io/crates/l/falkordb-mcp)](https://github.com/FalkorDB/falkordb-mcp?tab=License-1-ov-file)\
[![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/falkordb/falkordb-mcp)](https://github.com/FalkorDB/falkordb-mcp/issues)
[![Pipeline](https://img.shields.io/github/actions/workflow/status/falkordb/falkordb-mcp/main.yml)](https://github.com/FalkorDB/falkordb-mcp)
[![Codecov](https://codecov.io/gh/falkordb/falkordb-mcp/branch/main/graph/badge.svg)](https://codecov.io/gh/falkordb/falkordb-mcp)
[![Docs](https://img.shields.io/docsrs/falkordb-mcp)](https://docs.rs/falkordb-mcp/latest/falkordb_mcp/)\
[![Forum](https://img.shields.io/badge/Forum-falkordb-blue)](https://github.com/orgs/FalkorDB/discussions)
[![Discord](https://img.shields.io/discord/1146782921294884966?style=flat-square)](https://discord.com/invite/6M4QwDXn2w)

# falkordb-mcp

[![Try Free](https://img.shields.io/badge/Try%20Free-FalkorDB%20Cloud-FF8101?labelColor=FDE900&style=for-the-badge&link=https://app.falkordb.cloud)](https://app.falkordb.cloud)

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

## Example sessions

What actually happens between the assistant and this server. Each example starts with the user's
prompt, shows the tool calls the model makes (**▸ request** to the server, **◂ response** from it),
and ends with the answer the model gives back. The presentation is a **simplified transcript** — a
`▸ tool { arguments }` line and the response payload, rather than the literal MCP JSON-RPC envelope —
but the **response *content* is real**: the `◂` payloads were captured from the running server against
a demo `imdb` graph.

### 1. Answering a question from live data

> **You:** "Which movies did Keanu Reeves act in, and what were his roles?"

The model first reads the schema, so it uses the graph's *real* labels and property keys instead of
guessing:

```text
▸ get_schema   { "graph": "imdb" }
◂ {
    "labels": ["Actor", "Movie"],
    "relationship_types": ["ACTED_IN"],
    "property_keys": ["name", "born", "title", "year", "rating", "role"],
    "indexes": ["NODE :Actor(name)"],
    "constraints": []
  }
```

Now it writes a correct, **parameterized** read query (the name is a bound parameter, never spliced
into the Cypher) and gets real rows back:

```text
▸ query_read {
    "graph": "imdb",
    "cypher": "MATCH (a:Actor {name: $name})-[r:ACTED_IN]->(m:Movie) RETURN m.title AS title, m.year AS year, r.role AS role ORDER BY m.year",
    "params": { "name": "Keanu Reeves" }
  }
◂ {
    "columns": ["title", "year", "role"],
    "rows": [
      ["The Matrix", 1999, "Neo"],
      ["The Matrix Reloaded", 2003, "Neo"],
      ["John Wick", 2014, "John Wick"]
    ],
    "truncated": false
  }
```

> **Assistant:** "In this graph, Keanu Reeves acted in three movies: *The Matrix* (1999) and *The
> Matrix Reloaded* (2003) as **Neo**, and *John Wick* (2014) as **John Wick**."

**Value:** the answer is grounded in the live database and the real schema — not a guess, and not a
query the user has to run themselves.

### 2. Diagnosing a slow query

> **You:** "Why is `MATCH (m:Movie {title:'The Matrix'}) RETURN m` slow on a large graph?"

The model inspects the plan **without executing** the query:

```text
▸ explain {
    "graph": "imdb",
    "cypher": "MATCH (m:Movie {title:'The Matrix'}) RETURN m"
  }
◂ ["Results", "    Project", "        Filter", "            Node By Label Scan | (m:Movie)"]
```

> **Assistant:** "It does a full **`Node By Label Scan`** over every `Movie`, because there's no index
> on `Movie.title`. Add one — `CREATE INDEX FOR (m:Movie) ON (m.title)` — and the same query becomes a
> `Node By Index Scan`. (Your `Actor.name` lookups already use an index, so they're fast.)"

**Value:** the model reads the actual query plan and gives a concrete, correct optimization — the kind
of thing a static schema doc can't tell it.

### 3. Read-only by construction

> **You:** "Just delete all the `Actor` nodes to clean up."

Even if the model tries, the read tool runs through `GRAPH.RO_QUERY`, and the **server** refuses the
write:

```text
▸ query_read { "graph": "imdb", "cypher": "MATCH (a:Actor) DETACH DELETE a" }
◂ error: "An error occurred while sending the request to Redis: is to be executed only on read-only queries"
```

> **Assistant:** "I can't do that — this server is read-only, so FalkorDB rejected the delete. If you
> really want to allow writes, the operator has to start the server with `FALKORDB_MCP_ALLOW_WRITES=1`."

**Value:** safety is enforced by the database, not by trusting the model to behave. Writes are an
explicit operator opt-in (see [Writes](#writes-opt-in)).

### 4. Following relationships across the graph

> **You:** "Who has Keanu Reeves co-starred with?"

This is a multi-hop traversal — actor → movie ← other actor — the kind of join-of-joins that's awkward
in SQL but native to a graph:

```text
▸ query_read {
    "graph": "imdb",
    "cypher": "MATCH (k:Actor {name: $name})-[:ACTED_IN]->(:Movie)<-[:ACTED_IN]-(co:Actor) WHERE co <> k RETURN DISTINCT co.name AS costar ORDER BY costar",
    "params": { "name": "Keanu Reeves" }
  }
◂ {
    "columns": ["costar"],
    "rows": [["Carrie-Anne Moss"], ["Hugo Weaving"], ["Laurence Fishburne"]],
    "truncated": false
  }
```

> **Assistant:** "Keanu Reeves has co-starred with Carrie-Anne Moss, Hugo Weaving, and Laurence
> Fishburne."

**Value:** the model can walk relationships several hops deep against live data — the graph's core
strength — instead of approximating it from a static description.

### 5. Aggregating across the graph

> **You:** "Who are the most prolific actors in this graph?"

```text
▸ query_read {
    "graph": "imdb",
    "cypher": "MATCH (a:Actor)-[:ACTED_IN]->(m:Movie) RETURN a.name AS actor, count(m) AS movies ORDER BY movies DESC, actor"
  }
◂ {
    "columns": ["actor", "movies"],
    "rows": [["Keanu Reeves", 3], ["Carrie-Anne Moss", 2], ["Hugo Weaving", 1], ["Laurence Fishburne", 1]],
    "truncated": false
  }
```

> **Assistant:** "Keanu Reeves leads with 3 movies, followed by Carrie-Anne Moss with 2; Hugo Weaving
> and Laurence Fishburne have 1 each."

**Value:** analytics (counts, groupings, ordering) run directly on the current data — no export step, no
stale snapshot.

### 6. Returning whole nodes

> **You:** "Give me everything you have on *The Matrix*."

When a query returns a node (rather than scalar columns), the server serializes it as a structured DTO
with its `id`, `labels`, and `properties`:

```text
▸ query_read { "graph": "imdb", "cypher": "MATCH (m:Movie {title: 'The Matrix'}) RETURN m" }
◂ {
    "columns": ["m"],
    "rows": [[
      { "id": 4, "labels": ["Movie"], "properties": { "rating": 8.7, "title": "The Matrix", "year": 1999 } }
    ]],
    "truncated": false
  }
```

> **Assistant:** "*The Matrix* (1999) has a rating of 8.7."

**Value:** nodes, relationships, and paths come back as clean, predictable JSON the model can read
field-by-field — no bespoke parsing.

### 7. Making a change (writes opted in)

With `FALKORDB_MCP_ALLOW_WRITES=1`, the same gated path also exposes `query_write` and `profile`.

> **You:** "Add the movie *The Matrix Resurrections* (2021) with Keanu as Neo."

```text
▸ query_write {
    "graph": "imdb",
    "cypher": "MATCH (a:Actor {name: $actor}) CREATE (a)-[:ACTED_IN {role: $role}]->(m:Movie {title: $title, year: 2021}) RETURN m.title AS added",
    "params": { "actor": "Keanu Reeves", "role": "Neo", "title": "The Matrix Resurrections" }
  }
◂ { "columns": ["added"], "rows": [["The Matrix Resurrections"]], "truncated": false }
```

`profile` then *executes* a query and reports per-operation row counts and timings — useful for
confirming an index is actually used:

```text
▸ profile { "graph": "imdb", "cypher": "MATCH (a:Actor {name:'Keanu Reeves'})-[:ACTED_IN]->(m:Movie) RETURN m.title" }
◂ [
    "Results | Records produced: 4, Execution time: 0.000747 ms",
    "    Project | Records produced: 4, Execution time: 0.005751 ms",
    "        Conditional Traverse | (a)->(m:Movie) | Records produced: 4, Execution time: 0.231542 ms",
    "            Node By Index Scan | (a:Actor) | Records produced: 1, Execution time: 0.040667 ms"
  ]
```

> **Assistant:** "Added *The Matrix Resurrections* (2021) with Keanu as Neo. Keanu now has 4 movies, and
> the lookup uses the `Actor(name)` index (`Node By Index Scan`)."

**Value:** when an operator opts in, the assistant can make changes and measure real execution — still
through the same parameterized, gated interface.

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
- **No credentials in tool surface.** Connection details come only from the operator's environment,
  never from a tool call, and the startup connection error never echoes the connection URL.
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
