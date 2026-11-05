use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::fleet::model::ProviderName;
use crate::fleet::runtimes::Runtimes;

fn fleet(home: &Path) -> Fleet {
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap()
}

fn session(home: &Path, name: &str, role: &str, stopped: bool) {
    let directory = home.join(".local/state/north/fleet").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0, "host_pid": null, "conversation": name, "host_dir": "", "parent": null, "role": role, "stopped": stopped});
    std::fs::write(directory.join("session.json"), record.to_string()).unwrap();
}

fn worker(flags: &[&str], caller: Option<&str>) -> LaunchRequest {
    let words = [&["--provider", "claude", "--account", "c", "--name", "w", "--role", "worker"], flags, &["go"]].concat();
    super::super::launch::launch_parts(&[ProviderName::new("claude")], &words, caller.map(str::to_owned), false).unwrap().request
}

#[test]
fn a_worker_launched_with_no_parent_takes_its_live_fleet_launcher_as_parent_and_is_refused_from_outside_the_fleet_or_under_an_exited_parent() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    session(home.path(), "lead-a", "lead", false);
    session(home.path(), "gone", "lead", true);
    let mut adopted = worker(&["--parent", "none"], Some("lead-a"));
    reader(&fleet, &mut adopted, Some("lead-a")).unwrap();
    assert_eq!(adopted.parent.as_deref(), Some("lead-a"));
    let refused = reader(&fleet, &mut worker(&["--parent", "none"], None), None).unwrap_err().user_message();
    assert!(refused.contains("worker w has no parent to read its reports"), "{refused}");
    assert!(reader(&fleet, &mut worker(&["--parent", "none"], Some("gone")), Some("gone")).is_err(), "an exited launcher does not read reports");
    let exited = reader(&fleet, &mut worker(&["--parent", "gone"], Some("lead-a")), Some("lead-a")).unwrap_err().user_message();
    assert!(exited.contains("gone is not a live fleet session"), "{exited}");
    assert!(reader(&fleet, &mut worker(&["--parent", "nobody"], Some("lead-a")), Some("lead-a")).is_err());
    let mut named = worker(&["--parent", "lead-a"], None);
    reader(&fleet, &mut named, None).unwrap();
    assert_eq!(named.parent.as_deref(), Some("lead-a"));
    let mut lead = super::super::launch::launch_parts(&[ProviderName::new("claude")], &["--provider", "claude", "--account", "c", "--name", "l", "--parent", "none", "go"], None, false).unwrap().request;
    reader(&fleet, &mut lead, None).unwrap();
    assert_eq!(lead.parent, None, "a lead with no task needs no reader");
}

#[test]
fn only_a_session_in_the_proxy_role_launches_as_the_proxy() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    session(home.path(), "plato", "proxy", false);
    session(home.path(), "lead-a", "lead", false);
    assert!(from_proxy(&fleet, Some("proxy")) && from_proxy(&fleet, Some("plato")));
    assert!(!from_proxy(&fleet, Some("lead-a")) && !from_proxy(&fleet, None));
}
