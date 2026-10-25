//! The placement inputs `north dispatch` reads beside the gate: South's orchestration, the cloud
//! credit, the laptop's state, a host's resident CLIs and pace-control's state files.
use super::super::{sampling, storage};
use super::{credit, laptop, resident};
use serde_json::Value;
use std::path::PathBuf;
use toml_edit::DocumentMut;

/// South's orchestration.toml, which placement and `north dispatch` read.
pub(crate) fn config() -> Result<DocumentMut, String> {
    sampling::config()
}
pub(crate) fn cloud_credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    credit(cfg, at)
}
pub(crate) fn laptop_state() -> String {
    laptop()
}
pub(crate) fn resident_clis(name: &str, destination: &str) -> Option<i64> {
    resident(name, destination)
}
/// A JSON file pace-control keeps in ~/.local/state/agents, or `{}`.
pub(crate) fn pace_state(name: &str) -> Value {
    storage::load(&storage::directory().join(name))
}
pub(crate) fn pace_directory() -> PathBuf {
    storage::directory()
}
pub(crate) fn now() -> f64 {
    storage::now()
}
