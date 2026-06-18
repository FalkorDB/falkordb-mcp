# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial `falkordb-mcp` Model Context Protocol server (read-only v1): a stdio MCP server, built
  on the `rmcp` SDK and the async `falkordb` client, exposing the read-only tools `list_graphs`,
  `get_schema`, `query_read`, and `explain` so AI assistants can explore a live FalkorDB graph.
- Live FalkorDB backend: connects from `FALKORDB_URL`, runs reads through `GRAPH.RO_QUERY` (writes
  rejected server-side) with named JSON parameter binding, a server-side timeout, and an output row
  cap that flags `truncated`; results map to JSON via explicit DTOs; schema introspection covers
  labels, relationship types, property keys, indexes, and constraints.
- Opt-in guarded write tools, off by default: `query_write` (`GRAPH.QUERY`) and `profile`
  (`GRAPH.PROFILE`, which executes the query). They are registered — and even listed in `tools/list` —
  only when the operator sets `FALKORDB_MCP_ALLOW_WRITES=1`, and are re-checked at call time.
- Engineering scaffolding mirroring the `falkordb-rs` repo: a `just`-driven workflow, hermetic
  fake-backend tests plus opt-in live integration tests, and CI for fmt/clippy/build/doc/deny/test,
  coverage, spellcheck (Markdown + PR title), CodeQL, and release-plz publishing.
- `docs/upstream-falkordb-rs.md`: notes on `falkordb` client changes that would let the server drop a
  couple of workarounds (notably making `explain`'s future `Send`).
