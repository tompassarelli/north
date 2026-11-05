use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::{Value as Json, json};

use super::run;
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;

const SCHEMA: &str = include_str!("fixtures/ledger.sql");
const NOW: &str = "2026-10-28T09:00:00Z";

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    outside: Recorded,
}

/// wisp's issues carry labels, makgora's listing is unreadable, any other repo has none.
fn github(joined: &str) -> Option<Result<String, String>> {
    let reply = if joined.contains("-R tompassarelli/wisp") {
        json!([{"number": 3, "labels": [{"name": "bug"}]}, {"number": 9, "labels": [{"name": "enhancement"}, {"name": "theme:balance"}]}]).to_string()
    } else if joined.contains("-R tompassarelli/makgora") {
        "not json".to_owned()
    } else {
        "[]".to_owned()
    };
    Some(Ok(reply))
}

fn ledger() -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    Ledger { _directory: directory, path, outside: Recorded { respond: Some(github), prices: vec![("claude-b", 50.0), ("codex", 100.0)], ..Recorded::default() } }
}

impl Ledger {
    fn call(&self, argv: &[&str], stdin: &str) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: None, outside: &self.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut stdin.as_bytes(), &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn ingest(&self, rows: &[Json]) -> String {
        let (code, _, err) = self.call(&["ingest"], &rows.iter().map(|row| format!("{row}\n")).collect::<String>());
        assert_eq!(code, 0, "{err}");
        err
    }

    /// The query's one row, its columns joined by `|` the way the sqlite3 shell prints them.
    fn rows(&self, query: &str) -> Vec<Vec<Value>> {
        let db = Connection::open(&self.path).unwrap();
        let mut statement = db.prepare(query).unwrap();
        let width = statement.column_count();
        statement.query_map([], |row| (0..width).map(|index| row.get::<_, Value>(index)).collect()).unwrap().collect::<Result<_, _>>().unwrap()
    }

}

fn readout(provider: &str, account: &str, minute: u32, used: f64) -> Json {
    json!({"type": "usage", "ts": format!("2026-10-11T00:{minute:02}:00Z"), "provider": provider, "account": account, "kind": "window",
           "name": if provider == "codex" { "week" } else { "weekly_all" }, "used_pct": used, "resets_at": "2026-10-15T00:00:00Z"})
}

fn routine(id: &str, account: &str, samples: &[(u32, f64)]) -> Json {
    let samples: Vec<Json> = samples.iter().map(|(minute, weight)| json!({"request": format!("{id}-{minute}"), "ts": format!("2026-10-11T00:{minute:02}:00Z"), "weight": weight})).collect();
    json!({"id": id, "role": "routine", "account": account, "usage_samples": samples, "tier": "high", "actual_min": 20, "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:20:00Z", "outcome": "done", "unpriced_models": ["zeta-1", "alpha-2"]})
}

fn first_batch() -> Vec<Json> {
    vec![readout("claude", "b", 0, 10.0), readout("claude", "b", 10, 14.0), readout("claude", "b", 20, 20.0), readout("codex", "", 0, 10.0), readout("codex", "", 10, 13.0), routine("a", "claude-b", &[(5, 730.0)])]
}

fn second_batch() -> Vec<Json> {
    vec![readout("claude", "b", 30, 23.0), routine("b", "claude-b", &[(8, 270.0), (15, 100.0), (25, 30.0)]), routine("c", "codex", &[(5, 400.0)])]
}

#[test]
fn an_incremental_ingest_leaves_runs_and_run_costs_as_a_full_ingest_does() {
    let full = ledger();
    full.ingest(&[first_batch(), second_batch()].concat());
    let incremental = ledger();
    incremental.ingest(&first_batch());
    incremental.ingest(&second_batch());
    incremental.ingest(&[]);
    for sql in ["SELECT * FROM run_costs ORDER BY account, started, ended, agent", "SELECT * FROM runs ORDER BY agent"] {
        let expected = full.rows(sql);
        assert!(!expected.is_empty(), "{sql}");
        assert_eq!(incremental.rows(sql), expected, "{sql}");
    }
}

