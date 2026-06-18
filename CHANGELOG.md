# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial `falkordb-mcp` Model Context Protocol server (read-only v1): a stdio MCP server, built
  on the `rmcp` SDK and the async `falkordb` client, exposing the read-only tools `list_graphs`,
  `get_schema`, `query_read`, and `explain` so AI assistants can explore a live FalkorDB graph.
- Engineering scaffolding mirroring the `falkordb-rs` repo: a `just`-driven workflow, hermetic
  fake-backend tests, and CI for fmt/clippy/build/doc/deny/test, coverage, spellcheck (Markdown +
  PR title), CodeQL, and release-plz publishing.
