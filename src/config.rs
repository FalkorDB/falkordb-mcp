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
                .map(|v| {
                    matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                })
                .unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const URL: &str = "FALKORDB_URL";
    const MAX_ROWS: &str = "FALKORDB_MCP_MAX_ROWS";
    const ALLOW_WRITES: &str = "FALKORDB_MCP_ALLOW_WRITES";

    // `from_env` reads process-global env vars. libtest may run these tests concurrently in one
    // process, so serialize them on a single lock and clear the vars up front for determinism. The
    // lock is poison-tolerant so one failing test doesn't cascade into the others.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Lock the env for the test's duration and clear the vars `from_env` reads.
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for var in [URL, MAX_ROWS, ALLOW_WRITES] {
            std::env::remove_var(var);
        }
        guard
    }

    #[test]
    fn defaults_when_env_is_absent() {
        let _env = lock_env();
        let config = Config::from_env();
        assert_eq!(config.url, "falkor://127.0.0.1:6379");
        assert_eq!(config.max_rows, DEFAULT_MAX_ROWS);
        assert!(!config.allow_writes, "writes are off by default");
    }

    #[test]
    fn reads_overrides_from_env() {
        let _env = lock_env();
        std::env::set_var(URL, "falkor://db.internal:7000");
        std::env::set_var(MAX_ROWS, "50");
        let config = Config::from_env();
        assert_eq!(config.url, "falkor://db.internal:7000");
        assert_eq!(config.max_rows, 50);
    }

    #[test]
    fn invalid_max_rows_falls_back_to_default() {
        let _env = lock_env();
        std::env::set_var(MAX_ROWS, "0");
        assert_eq!(Config::from_env().max_rows, DEFAULT_MAX_ROWS);
        std::env::set_var(MAX_ROWS, "not-a-number");
        assert_eq!(Config::from_env().max_rows, DEFAULT_MAX_ROWS);
    }

    #[test]
    fn allow_writes_parses_truthy_values_case_insensitively() {
        let _env = lock_env();
        for v in ["1", "true", "TRUE", "Yes", "on", "ON"] {
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
    }
}
