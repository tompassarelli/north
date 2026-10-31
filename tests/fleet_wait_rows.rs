use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use north_core::fleet::records::write_record;
use north_core::fleet::store::{Store, WaitKind, WaitRow};
use north_core::foundation::clock::Timestamp;
use rusqlite::Connection;

const EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release','KICKOFF','ANSWER')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;";

struct Host {
    root: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let host = Self { root: tempfile::tempdir().unwrap() };
        Connection::open(host.ledger()).unwrap().execute_batch(EVENTS).unwrap();
        let session_dir = host.root.path().join(".local/state/north/fleet/c-test");
        fs::create_dir_all(&session_dir).unwrap();
        let record = serde_json::json!({"name": "c-test", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null});
        write_record(&session_dir, &serde_json::from_value(record).unwrap()).unwrap();
        host
    }

    fn ledger(&self) -> std::path::PathBuf {
        self.root.path().join("threads.db")
    }

    fn wait(&self, run: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["fleet", "wait", run])
            .current_dir(self.root.path())
            .env("HOME", self.root.path())
            .env("THREADS_DB", self.ledger())
            .env("NORTH_FLEET_SESSION", "c-test")
            .output()
            .unwrap()
    }

    fn waits(&self) -> Vec<WaitRow> {
        Store::open(Path::new(&self.ledger())).unwrap().waits().unwrap()
    }
}

fn seconds_between(wait: &WaitRow) -> i64 {
    Timestamp::parse(&wait.deadline).unwrap().unix_seconds() - Timestamp::parse(&wait.created).unwrap().unix_seconds()
}

#[test]
fn waiting_on_a_farm_run_a_branch_or_a_capture_writes_a_fleet_waits_row_with_a_default_deadline() {
    let host = Host::new();
    for run in ["farm:42", "branch:claude/land-x", "capture:/tmp/batch/done"] {
        let out = host.wait(run);
        assert!(out.status.success(), "{run}: {}", String::from_utf8_lossy(&out.stderr));
    }

    let waits = host.waits();
    let seen: Vec<(&str, WaitKind, &str)> = waits.iter().map(|wait| (wait.session.as_str(), wait.kind, wait.key.as_str())).collect();
    assert_eq!(seen, [("c-test", WaitKind::Branch, "claude/land-x"), ("c-test", WaitKind::Capture, "/tmp/batch/done"), ("c-test", WaitKind::Farm, "42")]);
    assert!(waits.iter().all(|wait| seconds_between(wait) >= 3600), "{waits:?}");
}

#[test]
fn waiting_on_a_job_or_a_process_writes_no_fleet_waits_row() {
    let host = Host::new();
    assert!(host.wait("job:soak").status.success());
    assert!(host.wait("pid:4242@99").status.success());
    assert!(host.waits().is_empty());
}
