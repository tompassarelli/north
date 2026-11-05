use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use north_core::fleet::sender::Sender;
use north_core::fleet::store::{FleetRow, JobPayload, RowKind};
use rusqlite::Connection;

const EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release','KICKOFF','ANSWER')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;";

fn ledger_at(home: &Path) {
    Connection::open(home.join("threads.db")).unwrap().execute_batch(EVENTS).unwrap();
}

fn north(home: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["fleet", "row"])
        .args(args)
        .env("HOME", home)
        .env("THREADS_DB", home.join("threads.db"))
        .env_remove("NORTH_FLEET_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn job_row(id: &str, kind: RowKind, key: &str) -> String {
    let job = JobPayload { key: key.into(), lease_secs: (kind != RowKind::JobExit).then_some(180), exit: (kind == RowKind::JobExit).then_some(0) };
    let row = FleetRow { id: id.into(), kind, to: "sc-1".into(), from: "far".into(), urgent: false, sent_at: 1_793_000_000_000, text: String::new(), sender: Sender::Tom, gate: None, answer: None, job: Some(job), origin: None };
    serde_json::to_string(&row).unwrap()
}

fn answer(home: &Path, session: &str) -> String {
    let output = north(home, &["leased", session], "");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn row_leased_answers_leased_from_a_jobs_start_until_its_exit_and_free_for_a_session_without_one() {
    let home = tempfile::tempdir().unwrap();
    ledger_at(home.path());
    assert_eq!(answer(home.path(), "sc-1"), "free");

    assert!(north(home.path(), &["put"], &job_row("j-1", RowKind::JobStart, "soak#1")).status.success());
    assert_eq!(answer(home.path(), "sc-1"), "leased");
    assert_eq!(answer(home.path(), "sc-2"), "free");

    assert!(north(home.path(), &["put"], &job_row("j-2", RowKind::JobExit, "soak#1")).status.success());
    assert_eq!(answer(home.path(), "sc-1"), "free");
}

#[test]
fn row_leased_without_a_session_is_a_usage_error_and_never_free() {
    let home = tempfile::tempdir().unwrap();
    let output = north(home.path(), &["leased"], "");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{}", String::from_utf8_lossy(&output.stdout));
}
