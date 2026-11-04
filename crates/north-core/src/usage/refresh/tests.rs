use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::record::{Reading, publish, record};
use super::{Ports, Readers, Request, promos, run};
use crate::foundation::clock::Timestamp;
use crate::usage::Places;

fn frozen() -> Timestamp {
    Timestamp::parse_rfc3339("2026-10-28T12:00:00Z").unwrap()
}

fn reading(ts: &str, name: &str, used: Value) -> Reading {
    json!({"ts": ts, "provider": "claude", "account": "a", "kind": "window", "name": name, "used_pct": used, "window_min": 300, "resets_at": null, "amount": null, "source": "test"})
        .as_object()
        .cloned()
        .unwrap()
}

fn count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM usage", [], |row| row.get(0)).unwrap()
}

fn plain_ssh(arguments: &[&str]) -> Vec<OsString> {
    std::iter::once("ssh").chain(arguments.iter().copied()).map(OsString::from).collect()
}

fn run_within(command: Command, limit: Duration) -> Option<Output> {
    crate::platform::process::run_command_within(command, None, limit)
}

fn readers<'a>() -> Readers<'a> {
    Readers { local: Vec::new(), machines: Vec::new(), here: "here".into(), ssh: &plain_ssh, run: &run_within }
}

fn script(path: &Path, body: &str) {
    crate::foundation::shim::write_executable(path, body);
}

#[test]
fn a_reading_already_recorded_for_its_window_and_time_is_not_recorded_twice() {
    let db = Connection::open_in_memory().unwrap();
    record(&db, &[reading("2026-10-28T11:00:00Z", "session", json!(3.5))]).unwrap();
    record(&db, &[reading("2026-10-28T11:00:00Z", "session", json!(9)), reading("2026-10-28T11:05:00Z", "session", json!(4))]).unwrap();
    let used: Vec<f64> = db.prepare("SELECT used_pct FROM usage ORDER BY ts").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(used, [3.5, 4.0]);
}

#[test]
fn a_mirrored_client_hands_its_readings_to_the_ledger_host_before_recording_them() {
    let sent = RefCell::new(Vec::new());
    let ingest = |batch: &[u8]| {
        sent.borrow_mut().push(String::from_utf8(batch.to_vec()).unwrap());
        Ok(())
    };
    let db = Connection::open_in_memory().unwrap();
    let rows = [reading("2026-10-28T11:00:00Z", "session", json!(3.5))];
    publish(&db, false, &rows, &ingest).unwrap();
    publish(&db, true, &[], &ingest).unwrap();
    assert!(sent.borrow().is_empty());
    publish(&db, true, &rows, &ingest).unwrap();
    let batch: Value = serde_json::from_str(sent.borrow()[0].trim()).unwrap();
    assert_eq!((batch["type"].as_str(), batch["name"].as_str(), count(&db)), (Some("usage"), Some("session"), 1));
}

#[test]
fn local_adapter_rows_are_collected_in_adapter_order() {
    let named = |name: &'static str| -> super::adapters::Local<'static> { Box::new(move || vec![json!({"provider": "p", "name": name}).as_object().cloned().unwrap()]) };
    let mut local = readers();
    local.local = vec![named("billing"), named("usage")];
    let names: Vec<Value> = local.read_all(false).into_iter().map(|row| row["name"].clone()).collect();
    assert_eq!(names, ["billing", "usage"]);
}

#[test]
fn a_remote_machine_supplies_only_the_rows_of_the_provider_it_is_listed_for() {
    let directory = tempfile::tempdir().unwrap();
    let ssh = directory.path().join("ssh");
    script(&ssh, "#!/bin/sh\necho '[{\"provider\": \"claude\", \"name\": \"week\", \"extra\": 1}, {\"provider\": \"gemini\", \"name\": \"other\"}]'\n");
    let fake = |arguments: &[&str]| [OsString::from("/bin/sh"), ssh.clone().into_os_string()].into_iter().chain(arguments.iter().map(OsString::from)).collect();
    let mut far = readers();
    far.machines = vec![("claude".into(), vec!["here".into(), "far".into()])];
    far.ssh = &fake;
    let rows = far.read_all(true);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["name"].as_str(), rows[0].get("extra"), rows[0]["resets_at"].is_null()), (Some("week"), None, true));
    assert!(far.read_all(false).is_empty());
}

fn catalog(end: &str) -> toml_edit::DocumentMut {
    format!("[accounts.claude]\nmonthly_price = 200\naccount_count = 3\n[accounts.gemini]\nmonthly_price = 20\npromo_price = 5\npromo_end = {end}\npromo_end_confirmed = false\n")
        .parse()
        .unwrap()
}

fn runs(tiers: &[&str]) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute("CREATE TABLE runs(tier TEXT, started TEXT)", []).unwrap();
    for tier in tiers {
        db.execute("INSERT INTO runs VALUES (?, '2026-10-20T00:00:00Z')", [tier]).unwrap();
    }
    db
}

#[test]
fn a_promotion_ending_within_a_week_asks_whether_its_run_share_earns_the_full_price() {
    let db = runs(&["gemini-3", "claude-opus", "claude-opus", "gpt-5"]);
    let lines = promos::lines(&catalog("2026-11-02"), &db, frozen()).unwrap();
    assert_eq!(
        lines,
        ["Blocked: decision for Tom: gemini promotion (USD 5/month) ends 2026-11-02 (4 days, unconfirmed); at USD 20/month it is 3.2% of plan spend and ran 25.0% of runs in 30 days (1 of 4): earned its full price; keep it at full price?"]
    );
    assert!(promos::lines(&catalog("2026-11-20"), &db, frozen()).unwrap().is_empty());
}

#[test]
fn a_promotion_notice_is_sent_once() {
    let state = tempfile::tempdir().unwrap();
    let sent = RefCell::new(0);
    let send = |_: &str, _: &str| *sent.borrow_mut() += 1;
    let lines = promos::lines(&catalog("2026-11-02"), &runs(&[]), frozen()).unwrap();
    promos::notify(&lines, state.path(), &send).unwrap();
    promos::notify(&lines, state.path(), &send).unwrap();
    assert_eq!((*sent.borrow(), state.path().join("agents/promo-notified/gemini-2026-11-02").is_file()), (1, true));
}

#[test]
fn a_refresh_records_what_the_adapters_read_and_rewrites_the_pace_gate() {
    let root = tempfile::tempdir().unwrap();
    let places = Places { policy: root.path().join("orchestration.toml"), operator: root.path().join("op"), state: root.path().join("state") };
    fs::write(&places.policy, "[accounts.claude.profiles.a]\nhere = {}\n").unwrap();
    let db = Connection::open_in_memory().unwrap();
    db.execute("CREATE TABLE runs(tier TEXT, started TEXT)", []).unwrap();
    let mut readers = readers();
    readers.local = vec![Box::new(|| vec![reading("2026-10-28T11:59:00Z", "session", json!(12.5))])];
    let ports = Ports { readers: &readers, ingest: &|_| Err("unexpected".into()), notify: &|_, _| {}, clock: &frozen };
    run(&db, &places, &Request { mirrored: false, remote: false }, &ports).unwrap();
    let gate: Value = serde_json::from_str(&fs::read_to_string(places.state.join("agents/usage-gate.json")).unwrap()).unwrap();
    assert_eq!((count(&db), gate["claude"]["pace_control"].clone()), (1, json!(true)));
}
