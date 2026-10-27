use serde_json::json;
use std::path::PathBuf;

pub(super) fn agents_directory() -> PathBuf {
    let base = match std::env::var_os("SPAWN_CAPACITY_HOME").filter(|home| !home.is_empty()) {
        Some(home) => PathBuf::from(home).join(".local/state"),
        None => std::env::var_os("XDG_STATE_HOME")
            .filter(|state| !state.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
            }),
    };
    base.join("agents")
}

pub(super) fn release_pending_use(session: &str, tool_use_id: &str) {
    let _ = crate::ops::pacing::answer(&json!({
        "op": "confirm",
        "directory": agents_directory(),
        "session": session,
        "tool_use_id": tool_use_id,
        "launched": false,
    }));
}
