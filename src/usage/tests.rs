use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

use super::report;
use super::store::{self, Source};
use crate::foundation::clock::Timestamp;

const STATES: &str = include_str!("fixtures/states.json");
const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn states() -> Vec<Value> {
    serde_json::from_str(STATES).unwrap()
}

fn ledger(path: &Path, rows: &Value) {
    let db = Connection::open(path).unwrap();
    db.execute(CREATE, []).unwrap();
    for row in rows.as_array().unwrap() {
        let values: Vec<rusqlite::types::Value> = row
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| match cell {
                Value::Null => rusqlite::types::Value::Null,
                Value::String(text) => rusqlite::types::Value::Text(text.clone()),
                number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
                number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
            })
            .collect();
        db.execute("INSERT INTO usage VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", rusqlite::params_from_iter(values)).unwrap();
    }
}

fn python(home: &Path, threads_db: Option<&Path>, policy: &Path) -> String {
    let mut command = Command::new("python3");
    command.current_dir(root()).args(["-I", "harness/lib/usage.py", "--json"]).env("HOME", home).env("AGENTS_ORCHESTRATION", policy);
    if let Some(path) = threads_db {
        command.env("THREADS_DB", path);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

fn rust(home: &Path, threads_db: Option<&Path>, policy: &Path) -> String {
    let source = Source::locate(home, threads_db.map(Into::into));
    report(&store::open(&source).unwrap(), policy, Timestamp::now()).unwrap()
}

#[test]
fn agents_usage_json_is_byte_equal_between_python_and_rust_on_three_recorded_sampler_states() {
    let states = states();
    assert_eq!(states.len(), 3);
    for state in &states {
        let scratch = tempfile::tempdir().unwrap();
        let (db, policy) = (scratch.path().join("threads.db"), scratch.path().join("orchestration.toml"));
        ledger(&db, &state["rows"]);
        fs::write(&policy, state["orchestration"].as_str().unwrap()).unwrap();
        let (expected, actual) = (python(scratch.path(), Some(&db), &policy), rust(scratch.path(), Some(&db), &policy));
        assert_eq!(actual, expected, "{}", state["name"]);
        assert!(serde_json::from_str::<Vec<Value>>(&actual).unwrap().len() > 1, "{}", state["name"]);
    }
}

#[test]
fn a_stale_flag_marks_readings_older_than_the_stale_minutes() {
    let scratch = tempfile::tempdir().unwrap();
    let (db, policy) = (scratch.path().join("threads.db"), scratch.path().join("orchestration.toml"));
    ledger(&db, &states()[1]["rows"]);
    fs::write(&policy, "").unwrap();
    let rows: Vec<Value> = serde_json::from_str(&rust(scratch.path(), Some(&db), &policy)).unwrap();
    let stale = |provider: &str| rows.iter().find(|row| row["provider"] == provider).map(|row| row["stale"].clone());
    assert_eq!((stale("claude"), stale("gemini")), (Some(true.into()), Some(false.into())));
}

#[test]
fn a_ledger_without_a_usage_table_or_file_prints_an_empty_list_like_python() {
    let scratch = tempfile::tempdir().unwrap();
    let policy = scratch.path().join("orchestration.toml");
    fs::write(&policy, "").unwrap();
    let bare = scratch.path().join("bare.db");
    Connection::open(&bare).unwrap().execute("CREATE TABLE other(x)", []).unwrap();
    for db in [bare, scratch.path().join("absent.db")] {
        assert_eq!((rust(scratch.path(), Some(&db), &policy), python(scratch.path(), Some(&db), &policy)), ("[]".into(), "[]".into()));
    }
}

#[test]
fn a_remote_host_reads_the_mirrored_snapshot_and_prints_what_python_prints() {
    let state = &states()[0];
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/threads")).unwrap();
    fs::write(home.path().join(".config/threads/remote"), "far\n").unwrap();
    fs::create_dir_all(home.path().join(".local/state/threads")).unwrap();
    ledger(&home.path().join(".local/state/threads/snapshot.db"), &state["rows"]);
    let policy = home.path().join("orchestration.toml");
    fs::write(&policy, state["orchestration"].as_str().unwrap()).unwrap();
    let actual = rust(home.path(), None, &policy);
    assert_eq!(actual, python(home.path(), None, &policy));
    assert!(actual.len() > 2);
}
