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

const RECORDED_BATCHES: &str = include_str!("fixtures/usage_batches.jsonl");
const RECORDED_PRICES: [(&str, f64); 2] = [("claude-a", 200.0), ("claude-c", 200.0)];

/// Recorded from the nexus ledger on 2026-11-06, run and request ids renamed: one ingest batch per
/// line, readouts in the order they were read and each run's usage samples arriving two batches late,
/// as a run's telemetry lands after the readouts it falls between.
fn recorded_batches() -> Vec<Vec<Json>> {
    RECORDED_BATCHES.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn recorded_ledger(prices: &[(&'static str, f64)]) -> Ledger {
    let mut ledger = ledger();
    ledger.outside.prices = prices.to_vec();
    ledger
}

fn earliest(batch: &[Json]) -> Timestamp {
    let stamps = batch.iter().flat_map(|row| row["usage_samples"].as_array().into_iter().flatten().chain([row]).filter_map(|row| row["ts"].as_str()));
    stamps.map(|ts| Timestamp::parse_python_datetime(ts).unwrap()).min().unwrap()
}

const RUN_COSTS: &str = "SELECT * FROM run_costs ORDER BY account, started, ended, agent";

#[test]
fn a_usage_batch_rewrites_only_the_intervals_its_rows_fall_in_and_leaves_run_costs_as_a_full_recompute_does() {
    let batches = recorded_batches();
    let full = recorded_ledger(&RECORDED_PRICES);
    full.ingest(&batches.concat());
    let mut incremental = recorded_ledger(&RECORDED_PRICES);
    let mut kept_earlier = 0;
    for (number, batch) in batches.iter().enumerate() {
        let db = Connection::open(&incremental.path).unwrap();
        db.execute_batch(
            "DROP TABLE IF EXISTS written; CREATE TABLE written(ended TEXT); \
             CREATE TRIGGER IF NOT EXISTS written_insert AFTER INSERT ON run_costs BEGIN INSERT INTO written VALUES (new.ended); END; \
             CREATE TRIGGER IF NOT EXISTS written_update AFTER UPDATE ON run_costs BEGIN INSERT INTO written VALUES (old.ended), (new.ended); END; \
             CREATE TRIGGER IF NOT EXISTS written_delete AFTER DELETE ON run_costs BEGIN INSERT INTO written VALUES (old.ended); END;",
        )
        .unwrap();
        incremental.ingest(batch);
        let since = earliest(batch);
        for row in incremental.rows("SELECT ended FROM written") {
            let Value::Text(ended) = &row[0] else { panic!("{row:?}") };
            assert!(Timestamp::parse_python_datetime(ended).unwrap() >= since, "batch {number} rewrote the interval ending {ended}, before its earliest row");
        }
        let untouched = incremental.rows(&format!("SELECT count(*) FROM run_costs WHERE ended < '{}'", since.iso()));
        if number > 0 && matches!(untouched[0][0], Value::Integer(count) if count > 0) {
            kept_earlier += 1;
        }
    }
    assert!(kept_earlier > batches.len() / 2, "only {kept_earlier} of {} batches left earlier intervals in place", batches.len());
    let expected = full.rows(RUN_COSTS);
    assert!(expected.len() > 50, "{}", expected.len());
    assert_eq!(incremental.rows(RUN_COSTS), expected);

    let repriced = [("claude-a", 120.0), ("claude-c", 200.0)];
    let late = vec![json!({"id": "run-0", "account": "claude-a", "telemetry_only": true, "usage_samples": [{"request": "req-late", "ts": "2026-11-05T12:49:59Z", "weight": 1.0}]})];
    incremental.outside.prices = repriced.to_vec();
    incremental.ingest(&late);
    let full = recorded_ledger(&repriced);
    full.ingest(&[batches.concat(), late].concat());
    assert_eq!(incremental.rows(RUN_COSTS), full.rows(RUN_COSTS), "a price change re-prices every interval at the next batch");
}
