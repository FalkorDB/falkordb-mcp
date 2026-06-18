//! Runtime configuration, read from the environment.

use crate::server::DEFAULT_MAX_ROWS;

/// Server configuration. All values come from the operator's environment (never from tool calls).
#[derive(Debug, Clone)]
pub struct Config {
    /// FalkorDB connection URL, e.g. `falkor://127.0.0.1:6379`. Defaults to localhost when unset.
    pub url: String,
    /// Maximum rows a single query may return.
    pub max_rows: usize,
    /// Whether the guarded write tools (`query_write`, `profile`) are exposed. Off unless explicitly
    /// enabled by the operator.
    pub allow_writes: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("FALKORDB_URL").unwrap_or_else(|_| "falkor://127.0.0.1:6379".into()),
            max_rows: std::env::var("FALKORDB_MCP_MAX_ROWS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0)
                .unwrap_or(DEFAULT_MAX_ROWS),
            allow_writes: std::env::var("FALKORDB_MCP_ALLOW_WRITES")
                .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
                .unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // nextest runs each test in its own process, so mutating these process-global env vars is
    // isolated between tests.
    const URL: &str = "FALKORDB_URL";
    const MAX_ROWS: &str = "FALKORDB_MCP_MAX_ROWS";
    const ALLOW_WRITES: &str = "FALKORDB_MCP_ALLOW_WRITES";

    #[test]
    fn defaults_when_env_is_absent() {
        std::env::remove_var(URL);
        std::env::remove_var(MAX_ROWS);
        std::env::remove_var(ALLOW_WRITES);
        let config = Config::from_env();
        assert_eq!(config.url, "falkor://127.0.0.1:6379");
        assert_eq!(config.max_rows, DEFAULT_MAX_ROWS);
        assert!(!config.allow_writes, "writes are off by default");
    }

    #[test]
    fn reads_overrides_from_env() {
        std::env::set_var(URL, "falkor://db.internal:7000");
        std::env::set_var(MAX_ROWS, "50");
        let config = Config::from_env();
        assert_eq!(config.url, "falkor://db.internal:7000");
        assert_eq!(config.max_rows, 50);
    }

    #[test]
    fn invalid_max_rows_falls_back_to_default() {
        std::env::remove_var(URL);
        std::env::set_var(MAX_ROWS, "0");
        assert_eq!(Config::from_env().max_rows, DEFAULT_MAX_ROWS);
        std::env::set_var(MAX_ROWS, "not-a-number");
        assert_eq!(Config::from_env().max_rows, DEFAULT_MAX_ROWS);
    }

    #[test]
    fn allow_writes_parses_truthy_values() {
        for v in ["1", "true", "yes", "on"] {
            std::env::set_var(ALLOW_WRITES, v);
            assert!(Config::from_env().allow_writes, "{v} should enable writes");
        }
        for v in ["0", "false", "no", ""] {
            std::env::set_var(ALLOW_WRITES, v);
            assert!(
                !Config::from_env().allow_writes,
                "{v:?} should not enable writes"
            );
        }
        std::env::remove_var(ALLOW_WRITES);
    }
}
