use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(10);

fn fleet_list(home: &Path) -> Output {
    let child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["fleet", "list", "--json"])
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env_remove("NORTH_SESSION")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let pid = child.id();
    let waiter = std::thread::spawn(move || child.wait_with_output().unwrap());
    while !waiter.is_finished() {
        if started.elapsed() > PATIENCE {
            Command::new("kill").arg(pid.to_string()).status().unwrap();
            panic!("north fleet list --json still waiting on the ledger after {PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    waiter.join().unwrap()
}

fn listed_names(output: &Output) -> Vec<String> {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let sessions: Value = serde_json::from_slice(&output.stdout).unwrap();
    sessions.as_array().unwrap().iter().map(|session| session["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn fleet_list_json_returns_the_fleet_while_another_process_holds_a_ledger_write() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path();
    let session = home.join(".local/state/north/fleet/lane-gc-probe");
    fs::create_dir_all(&session).unwrap();
    let record = json!({"name": "lane-gc-probe", "provider": "claude", "account": "a", "cwd": home, "model": null, "created_ms": 1, "host_pid": null, "conversation": null});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    assert_eq!(listed_names(&fleet_list(home)), ["lane-gc-probe"]);

    let writer = Connection::open(home.join(".local/state/north/ledger.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE; INSERT INTO events(session, kind, payload, recorded_ms) VALUES('writer', 'held', '{}', 1)").unwrap();
    let listed = fleet_list(home);
    writer.execute_batch("COMMIT").unwrap();

    assert_eq!(listed_names(&listed), ["lane-gc-probe"]);
    assert!(!String::from_utf8_lossy(&listed.stderr).contains("locked"), "{}", String::from_utf8_lossy(&listed.stderr));
}
