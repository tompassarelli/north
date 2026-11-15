use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::Role;
use super::runtime::PROXY;
use super::sender::Sender;

pub fn fleet_dir() -> PathBuf {
    std::env::var_os("NORTH_FLEET_DIR").map_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/state/north/fleet"), PathBuf::from)
}

pub fn grounded(fleet: &Path, named: Option<&str>, conversation: &str, entrypoint: Option<&str>) -> bool {
    match record(fleet, named.filter(|name| !name.is_empty()), conversation) {
        Some(record) => (role(&record) == Some(Role::Proxy) || record["name"] == PROXY) && (adopted(&record) || sender(&record) == Some(Sender::Tom)),
        None => entrypoint == Some("cli"),
    }
}

fn record(fleet: &Path, named: Option<&str>, conversation: &str) -> Option<Value> {
    if let Some(name) = named {
        let valid = !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.@-".contains(c));
        return Some(valid.then(|| read(&fleet.join(name).join("session.json"))).flatten().unwrap_or(Value::Null));
    }
    if conversation.is_empty() {
        return None;
    }
    std::fs::read_dir(fleet).into_iter().flatten().flatten().filter_map(|entry| read(&entry.path().join("session.json"))).find(|record| record["conversation"].as_str() == Some(conversation))
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn role(record: &Value) -> Option<Role> {
    serde_json::from_value(record.get("role")?.clone()).ok()
}

fn adopted(record: &Value) -> bool {
    record.get("adopted_ms").is_some_and(|at| !at.is_null())
}

fn sender(record: &Value) -> Option<Sender> {
    serde_json::from_value(record.get("prompt_sender")?.clone()).ok()
}
