//! The placement inputs `north dispatch` reads beside the gate: South's orchestration, the cloud
//! credit, the laptop's state, a host's resident CLIs and pace-control's state files.
use super::super::{sampling, storage};
use super::{credit, laptop, resident};
use serde_json::Value;
use std::path::PathBuf;
use toml_edit::DocumentMut;

/// South's orchestration.toml, which placement and `north dispatch` read.
pub fn config() -> Result<DocumentMut, String> {
    sampling::config()
}
pub fn cloud_credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    credit(cfg, at)
}
pub fn laptop_state() -> String {
    laptop()
}
pub fn resident_clis(name: &str, destination: &str) -> Option<i64> {
    resident(name, destination)
}
/// A JSON file pace-control keeps in ~/.local/state/agents, or `{}`.
pub fn pace_state(name: &str) -> Value {
    storage::load(&storage::directory().join(name))
}
pub fn pace_directory() -> PathBuf {
    storage::directory()
}
pub fn now() -> f64 {
    storage::now()
}
