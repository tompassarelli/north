use super::*;
use crate::fleet::model::{LaunchRequest, Machine};

fn record(name: &str, parent: Option<&str>, task: Option<&str>, login: Option<&str>) -> SessionRecord {
    let request: LaunchRequest = serde_json::from_value(serde_json::json!({
        "verification": false, "name": name, "provider": "claude", "account": "a", "cwd": "/home/op/code/north/main", "model": null, "prompt": "", "system_prompt": null,
        "foreground_cap_secs": null, "prompt_mode": "north", "idle_exit_minutes": null, "parent": parent, "task_ref": task, "placement": null, "machine": null, "role": null
    }))
    .unwrap();
    let mut record = SessionRecord::launched(&request, None, None, false, 0);
    record.machine = login.map(|login| Machine { name: "laptop".to_owned(), ssh: "op@host".to_owned(), agent_ssh: Some(login.to_owned()) });
    record.brief = Some(PathBuf::from("/home/op/briefs/b.md"));
    record
}

fn lead() -> SessionRecord {
    record("lead", None, Some("north#1"), None).tap_role(Role::Lead)
}

fn writer(dir: &Path) -> StartWriter {
    StartWriter::new(dir, Path::new("/home/op"))
}

#[test]
fn a_local_agent_launch_of_a_lead_with_a_task_writes_exactly_the_four_fields_the_starter_reads() {
    let dir = tempfile::tempdir().unwrap();
    let launch = record("child", Some("lead"), Some("north#1"), Some("agent@.host"));
    let path = writer(dir.path()).write(&launch, Some(&lead())).unwrap().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value, serde_json::json!({"name": "child", "account": "a", "cwd": "/home/agent/code/north/main", "brief": "/home/agent/briefs/b.md"}));
    StartRequest::parse(&bytes, Path::new("/home/agent")).unwrap();
    let other = tempfile::tempdir().unwrap();
    for login in [None, Some("agent@cpx62")] {
        assert_eq!(writer(other.path()).write(&record("child", Some("lead"), Some("north#1"), login), Some(&lead())), Ok(None));
    }
    let askers = [None, Some(record("lead", None, None, None).tap_role(Role::Lead)), Some(record("lead", Some("boss"), Some("north#1"), None).tap_role(Role::Worker)), Some(record("lead", None, Some("north#1"), None).tap_role(Role::Proxy))];
    for asker in &askers {
        assert!(writer(other.path()).write(&launch, asker.as_ref()).is_err(), "{asker:?}");
    }
    assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
}

trait TapRole {
    fn tap_role(self, role: Role) -> Self;
}

impl TapRole for SessionRecord {
    fn tap_role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }
}

#[test]
fn a_field_the_starter_would_refuse_is_refused_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut api = record("child", Some("lead"), Some("north#1"), Some("agent@.host"));
    api.account = "api".to_owned();
    let mut outside = record("child", Some("lead"), Some("north#1"), Some("agent@.host"));
    outside.brief = Some(PathBuf::from("/etc/passwd"));
    let mut odd = record("-child", Some("lead"), Some("north#1"), Some("agent@.host"));
    odd.name = "-child".to_owned();
    for launch in [api, outside, odd] {
        assert!(writer(dir.path()).write(&launch, Some(&lead())).is_err());
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn the_request_is_staged_under_a_name_the_starter_does_not_read_and_renamed_into_place() {
    let dir = tempfile::tempdir().unwrap();
    let launch = record("child", Some("lead"), Some("north#1"), Some("agent@.host"));
    let missing = dir.path().join("absent");
    assert!(writer(&missing).write(&launch, Some(&lead())).is_err());
    let starter = north_agent_starts::Requests::at(dir.path());
    let (request, _) = writer(dir.path()).request(&launch, Some(&lead())).unwrap().unwrap();
    let staged = writer(dir.path()).stage(&request).unwrap();
    assert_eq!(starter.waiting().unwrap(), Vec::<PathBuf>::new(), "a staged request is invisible to the starter");
    assert_eq!(std::fs::read(staged.path()).unwrap(), request.line().into_bytes(), "it is complete before it is placed");
    let placed = dir.path().join("child.start");
    StartWriter::place(staged, &placed).unwrap();
    assert_eq!(starter.waiting().unwrap(), vec![placed]);
    std::fs::remove_file(dir.path().join("child.start")).unwrap();
    let path = writer(dir.path()).write(&launch, Some(&lead())).unwrap().unwrap();
    assert_eq!(starter.waiting().unwrap(), vec![path]);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary file is left beside the request");
    assert_eq!(writer(dir.path()).write(&launch, Some(&lead())), Ok(None), "a request still waiting is not written twice");
}
