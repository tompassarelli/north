use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::store::{self, Source};
use super::Places;
use crate::foundation::clock::Timestamp;

const STATES: &str = include_str!("fixtures/states.json");
const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";
const FROZEN: &str = "2026-10-28T12:00:00Z";

struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new(state: &Value) -> Self {
        let scratch = Self { dir: tempfile::tempdir().unwrap() };
        ledger(&scratch.db(), &state["rows"]);
        fs::write(scratch.places().policy, state["orchestration"].as_str().unwrap_or_default()).unwrap();
        if !state["web"].is_null() {
            fs::create_dir_all(scratch.places().state.join("agents/usage")).unwrap();
            fs::write(scratch.places().state.join("agents/usage/claude-web.json"), state["web"].to_string()).unwrap();
        }
        scratch
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("threads.db")
    }

    fn places(&self) -> Places {
        let root = self.dir.path();
        Places { policy: root.join("orchestration.toml"), operator: root.join("op"), state: root.join("state") }
    }

    fn open(&self) -> Connection {
        store::open(&Source::locate(self.dir.path(), Some(self.db().into())), &|| {}).unwrap()
    }
}

fn frozen() -> Timestamp {
    Timestamp::parse_rfc3339(FROZEN).unwrap()
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

// KEEP(bug): a corrupt snapshot is refreshed once, then reported without a second pull; regression of the snapshot-recovery fix (ed9bb0cfd)
#[test]
fn a_corrupt_snapshot_is_refreshed_once_and_persistent_corruption_is_reported_after_that_one_retry() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/threads")).unwrap();
    fs::write(home.path().join(".config/threads/remote"), "far\n").unwrap();
    fs::create_dir_all(home.path().join(".local/state/threads")).unwrap();
    let snapshot = home.path().join(".local/state/threads/snapshot.db");
    let good = home.path().join("canonical.db");
    ledger(&good, &states()[0]["rows"]);
    let corrupt = || fs::write(&snapshot, [b"SQLite format 3\0".as_slice(), &[b'x'; 600]].concat()).unwrap();
    let source = Source::locate(home.path(), None);

    corrupt();
    let pulls = Cell::new(0);
    let repaired = store::open(&source, &|| {
        pulls.set(pulls.get() + 1);
        fs::copy(&good, &snapshot).unwrap();
    })
    .unwrap();
    assert_eq!(pulls.get(), 1);
    assert!(!store::latest(&repaired).unwrap().is_empty());

    corrupt();
    let before = fs::read(&snapshot).unwrap();
    pulls.set(0);
    let error = store::open(&source, &|| pulls.set(pulls.get() + 1)).unwrap_err();
    assert!(error.starts_with("NorthThreads: snapshot still unreadable after one refresh"), "{error}");
    assert_eq!((pulls.get(), fs::read(&snapshot).unwrap()), (1, before));
}

const CTX_OLD: &str = include_str!("fixtures/ctx_python.json");

fn same(left: &Value, right: &Value, path: &str) {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "{path}");
            a.iter().for_each(|(key, value)| same(value, &b[key], &format!("{path}.{key}")));
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            a.iter().zip(b).enumerate().for_each(|(index, (x, y))| same(x, y, &format!("{path}[{index}]")));
        }
        (Value::Number(a), Value::Number(b)) if a.is_f64() && b.is_f64() => {
            assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-6, "{path}: {a} vs {b}");
        }
        _ => assert_eq!(left, right, "{path}"),
    }
}

// KEEP(bug): only test of the spawn gate's ctx builder over a recorded ledger
#[test]
fn the_ctx_built_for_the_router_equals_the_one_the_python_plan_usage_built_on_a_recorded_ledger_with_runs_and_model_events() {
    let state: Value = serde_json::from_str(include_str!("fixtures/gate_state.json")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (db_path, policy) = (scratch.path().join("threads.db"), scratch.path().join("orchestration.toml"));
    ledger(&db_path, &state["rows"]);
    let db = Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, tier TEXT, category TEXT, outcome TEXT, expects_landing INTEGER, landed INTEGER, item TEXT, role TEXT, follows TEXT); CREATE TABLE model_events(model TEXT, at TEXT, p REAL, kind TEXT, ts TEXT);").unwrap();
    for (sql, rows) in [("INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", &state["runs"]), ("INSERT INTO model_events VALUES (?1, ?2, ?3, ?4, ?5)", &state["events"])] {
        for row in rows.as_array().unwrap() {
            let cells = row.as_array().unwrap().iter().map(|cell| match cell {
                Value::Null => rusqlite::types::Value::Null,
                Value::String(text) => rusqlite::types::Value::Text(text.clone()),
                number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
                number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
            });
            db.execute(sql, rusqlite::params_from_iter(cells)).unwrap();
        }
    }
    fs::write(&policy, include_str!("../router/fixtures/policy/orchestration.toml")).unwrap();
    let expected: Value = serde_json::from_str(CTX_OLD).unwrap();
    let inputs = super::gate::Inputs::load(&policy).unwrap();
    let readings = super::readings(&db, &policy).unwrap();
    for provider in ["claude", "gemini"] {
        let mut actual = inputs.context(&db, &readings, provider, frozen()).unwrap();
        actual.as_object_mut().unwrap().remove("now");
        same(&expected[provider], &actual, provider);
    }
    assert_eq!(expected["claude"]["success"], serde_json::json!({"claude:haiku|tooling": 60}));
    assert_eq!(expected["claude"]["tiers"][0]["successor"]["model"], "claude-haiku-5-7");
}

// KEEP(bug): an expired window that a newer reading of the account postdates reads 0% used, so a setup-token account is not held stale at the spawn gate forever; regression of north#401
#[test]
fn a_window_a_newer_reading_of_the_account_postdates_the_reset_of_reads_zero_used_as_of_that_reading() {
    let window = |reading_ts: &str| {
        let rows = serde_json::json!([
            ["2026-10-28T09:00:00Z", "claude", "a", "window", "session", 30.0, 300, "2026-10-28T10:00:00Z", null, "headers"],
            [reading_ts, "claude", "a", "window", "weekly_all", 80.0, 10080, "2026-11-03T00:00:00Z", null, "headers"],
        ]);
        let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": include_str!("../router/fixtures/policy/orchestration.toml")}));
        let gate = super::gate::write(&scratch.open(), &["claude"], &scratch.places(), frozen()).unwrap();
        gate["claude"]["projections"]["claude:a"].as_array().unwrap().iter().find(|w| w["window"] == "5h").unwrap().clone()
    };
    let rolled = window("2026-10-28T11:59:00Z");
    assert_eq!((rolled["used_pct"].as_f64(), rolled["as_of"].as_str(), rolled["age_s"].as_f64()), (Some(0.0), Some("2026-10-28T11:59:00Z"), Some(60.0)), "{rolled}");
    let unseen = window("2026-10-28T09:30:00Z");
    assert_eq!((unseen["used_pct"].as_f64(), unseen["as_of"].as_str()), (Some(30.0), Some("2026-10-28T09:00:00Z")), "{unseen}");
}

// KEEP(bug): a window a refresh did not reread cannot hold a freshly read account stale at the spawn gate; regression of d24e1dccd (north#299)
#[test]
fn one_window_a_refresh_did_not_reread_cannot_hold_a_freshly_read_account_stale_at_the_spawn_gate() {
    let refusal = |fable_ts: &str, fresh_ts: &str| {
        let rows = serde_json::json!([
            [fresh_ts, "claude", "a", "window", "session", 20.0, 300, "2026-10-28T14:00:00Z", null, "headers"],
            [fresh_ts, "claude", "a", "window", "weekly_all", 10.0, 10080, "2026-11-03T00:00:00Z", null, "headers"],
            [fable_ts, "claude", "a", "window", "weekly_scoped/Fable", 0.0, 10080, "2026-11-03T00:00:00Z", null, "web"],
            [fable_ts, "claude", "a", "billing", "extra_usage", null, null, null, 0.0, "web"],
        ]);
        let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": include_str!("../router/fixtures/policy/orchestration.toml")}));
        let gate = super::gate::write(&scratch.open(), &["claude"], &scratch.places(), frozen()).unwrap();
        gate["claude"]["refusals"]["claude"].clone()
    };
    assert_eq!(refusal("2026-10-28T10:39:00Z", "2026-10-28T11:59:00Z"), Value::Null);
    let held = refusal("2026-10-28T10:39:00Z", "2026-10-28T10:39:00Z");
    assert!(held.as_str().is_some_and(|why| why.contains("usage snapshot is 81 minutes old even after a refresh")), "{held}");
}
