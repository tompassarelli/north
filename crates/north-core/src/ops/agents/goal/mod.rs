//! The goal planner: the critical path over a project's open Done-when boxes, the quote for N boxes, and the score of a passed target.
mod actual;
mod boxes;
mod estimate;
mod metrics;
mod plan;
mod quote;
mod schedule;
mod score;
#[cfg(test)]
mod tests;

use crate::foundation::clock::Timestamp;

pub use plan::plan_lines;
pub use quote::run as quote;
pub use score::run as score;

pub(super) fn stamp(text: &str) -> Option<Timestamp> {
    Timestamp::parse_loose(text)
}

pub(super) fn plus_minutes(at: Timestamp, minutes: f64) -> Timestamp {
    Timestamp::from_unix_nanos(at.unix_nanos() + (minutes * 60e9) as i128)
}

pub(super) fn span(minutes: f64) -> String {
    let minutes = minutes.ceil() as i64;
    if minutes < 60 { format!("{minutes}m") } else { format!("{}h{:02}m", minutes / 60, minutes % 60) }
}

pub(super) fn state_dir() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()).map_or_else(|| home.join(".local/state"), Into::into)
}

pub(super) fn snapshot_path() -> std::path::PathBuf {
    std::env::var_os("THREADS_DB").filter(|path| !path.is_empty()).map_or_else(|| state_dir().join("threads/snapshot.db"), Into::into)
}

pub fn read_config(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(crate::router::policy::json_value(document.as_item()))
}
