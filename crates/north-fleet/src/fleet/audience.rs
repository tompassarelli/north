use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::Role;
use super::sender::Sender;

pub fn fleet_dir() -> PathBuf {
    std::env::var_os("NORTH_FLEET_DIR").map_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/state/north/fleet"), PathBuf::from)
}

pub fn grounded(fleet: &Path, named: Option<&str>, conversation: &str, entrypoint: Option<&str>) -> bool {
    match record(fleet, named.filter(|name| !name.is_empty()), conversation) {
        Some(record) => role(&record) == Some(Role::Proxy) && sender(&record) == Some(Sender::Tom),
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

fn sender(record: &Value) -> Option<Sender> {
    serde_json::from_value(record.get("prompt_sender")?.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::grounded;

    fn fleet(records: &[(&str, &str, &str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, role, sender, conversation) in records {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
            let record = serde_json::json!({"name": name, "conversation": conversation, "role": role, "prompt_sender": sender});
            std::fs::write(dir.path().join(name).join("session.json"), serde_json::to_string_pretty(&record).unwrap()).unwrap();
        }
        dir
    }

    #[test]
    fn a_role_proxy_fleet_session_is_grounded_for_a_tom_message_found_by_conversation_or_by_name() {
        let dir = fleet(&[("proxy", "proxy", "tom", "p1")]);
        assert!(grounded(dir.path(), None, "p1", Some("sdk-cli")));
        assert!(grounded(dir.path(), Some("proxy"), "other", Some("sdk-cli")));
    }

    #[test]
    fn a_role_proxy_fleet_session_is_not_grounded_for_a_service_or_session_message() {
        for sender in ["service:north-fleet", "session:lead"] {
            let dir = fleet(&[("proxy", "proxy", sender, "p1")]);
            assert!(!grounded(dir.path(), None, "p1", Some("cli")), "{sender} by conversation");
            assert!(!grounded(dir.path(), Some("proxy"), "p1", Some("cli")), "{sender} by name");
        }
    }

    #[test]
    fn a_worker_or_lead_fleet_session_is_never_grounded_even_for_a_tom_message() {
        for role in ["worker", "lead"] {
            let dir = fleet(&[("child", role, "tom", "c1")]);
            assert!(!grounded(dir.path(), None, "c1", Some("cli")), "{role} by conversation");
            assert!(!grounded(dir.path(), Some("child"), "c1", Some("cli")), "{role} by name");
        }
    }

    #[test]
    fn outside_the_fleet_only_the_interactive_cli_is_grounded() {
        let dir = fleet(&[("child", "worker", "tom", "c1")]);
        assert!(grounded(dir.path(), None, "s", Some("cli")));
        assert!(!grounded(dir.path(), None, "s", Some("sdk-cli")));
        assert!(!grounded(dir.path(), None, "s", None));
    }

    #[test]
    fn a_named_session_without_a_readable_record_or_with_an_unsafe_name_is_not_grounded() {
        let dir = fleet(&[("proxy", "proxy", "tom", "p1")]);
        assert!(!grounded(dir.path(), Some("missing"), "p1", Some("cli")));
        assert!(!grounded(dir.path(), Some("../proxy"), "p1", Some("cli")));
        assert!(!grounded(dir.path(), Some(".hidden"), "p1", Some("cli")));
    }
}
