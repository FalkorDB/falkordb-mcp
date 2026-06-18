//! `falkordb-mcp` — a Model Context Protocol server giving AI assistants read-only access to a live
//! FalkorDB graph database (list graphs, read the schema, run read-only Cypher, and explain queries).

pub mod backend;
pub mod config;
pub mod falkor;
pub mod server;
