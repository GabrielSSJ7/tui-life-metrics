use std::path::PathBuf;

/// SQLite DB path: `$TUI_LIFE_METRICS_DB` if set, else `<data_dir>/metrics.db`.
pub fn db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("TUI_LIFE_METRICS_DB") {
        return PathBuf::from(p);
    }
    data_dir().join("metrics.db")
}

/// Data root: `$TUI_LIFE_METRICS_DIR` if set, else `~/.local/tui-life-metrics`.
///
/// The caller is responsible for creating it (`std::fs::create_dir_all`).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("TUI_LIFE_METRICS_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").expect("HOME env var not set");
    PathBuf::from(home).join(".local/tui-life-metrics")
}
