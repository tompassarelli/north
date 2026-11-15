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
fn a_usage_batch_re_prices_only_the_intervals_from_its_rows_through_the_next_rise_and_leaves_run_costs_as_a_full_recompute_does() {
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

    let late_reading = vec![json!({"type": "usage", "ts": "2026-11-05T12:05:30Z", "provider": "claude", "account": "a", "kind": "window", "name": "weekly_all", "used_pct": 55.0, "resets_at": "2026-11-12T02:00:00Z"})];
    let mark = "UPDATE run_costs SET cost_usd = -1 WHERE rowid = (SELECT min(rowid) FROM run_costs WHERE account = 'claude-a' AND ended BETWEEN ?1 AND ?2)";
    let marked = |from: &str, to: &str| incremental.rows(&format!("SELECT count(*) FROM run_costs WHERE cost_usd = -1 AND ended BETWEEN '{from}' AND '{to}'"));
    let db = Connection::open(&incremental.path).unwrap();
    assert_eq!(db.execute(mark, ["2026-11-05T12:14:33Z", "2026-11-05T12:14:33Z"]).unwrap() + db.execute(mark, ["2026-11-05T12:15Z", "2026-11-06"]).unwrap(), 2);
    incremental.ingest(&late_reading);
    assert_eq!(marked("2026-11-05T12:14:33Z", "2026-11-05T12:14:33Z"), [[Value::Integer(0)]], "a late reading re-prices the interval its plateau ends in");
    assert_eq!(marked("2026-11-05T12:15Z", "2026-11-06"), [[Value::Integer(1)]], "a late reading leaves the intervals after the next rise as stored");

    let repriced = [("claude-a", 120.0), ("claude-c", 200.0)];
    let late = vec![json!({"id": "run-0", "account": "claude-a", "telemetry_only": true, "usage_samples": [{"request": "req-late", "ts": "2026-11-05T12:49:59Z", "weight": 1.0}]})];
    incremental.outside.prices = repriced.to_vec();
    incremental.ingest(&late);
    let full = recorded_ledger(&repriced);
    full.ingest(&[batches.concat(), late_reading, late].concat());
    assert_eq!(incremental.rows(RUN_COSTS), full.rows(RUN_COSTS), "a price change re-prices every interval at the next batch");
}

/// On 2026-11-08 north-719-lead held north#719 while it staffed children on its boxes; the ingest of a child's running run
/// took the lead's whole-issue claim, so every sibling's box launch was refused as owned by the child's conversation.
/// On 2026-11-13 a worker briefed `Item: none` read north#1247 with `gh issue view`, and its running run claimed the whole issue.
#[test]
fn a_running_worker_leaves_a_claim_its_lead_holds_and_claims_only_the_unheld_issue_its_brief_names() {
    let ledger = ledger();
    ledger.ingest(&[]);
    Connection::open(&ledger.path).unwrap().execute("INSERT INTO claims(item, owner, since, eta_min) VALUES ('north#719', 'north-719-lead', '2026-11-07T18:52:26Z', 120)", []).unwrap();
    let child = |id: &str, item: &str| json!({"id": id, "role": "worker", "status": "running", "tier": "medium", "started": "2026-11-08T02:24:00Z", "eta_min": 60, "brief": format!("Item: {item}\nCategory: feature\nSpec: exact\nScope: module\nSurface: rust\nVerify: local-test\n")});
    ledger.ingest(&[child("7bb646ee", "north#719"), child("c2", "north#720"), json!({"id": "88e8f4a8", "role": "worker", "status": "running", "tier": "medium", "started": "2026-11-13T10:54:28Z", "refs": ["north#1247"], "brief": "Item: none\nCategory: bug-known-cause\nSpec: exact\nScope: module\nSurface: rust\nVerify: local-test\n"})]);
    assert_eq!(
        ledger.rows("SELECT item, owner FROM claims ORDER BY item"),
        [["north#719", "north-719-lead"], ["north#720", "c2"]].map(|row| row.map(|text| Value::Text(text.to_owned())).to_vec())
    );
}

/// A worker claimed two boxes of north#872, ticked the first citing the main commit its Done-when check passed on, and
/// left the second open; it landed nothing, so its run read `not done` (north#872).
#[test]
fn settle_split_reads_the_boxes_a_session_ticked_beside_the_boxes_it_left_open() {
    let ledger = ledger();
    let brief = "Item: north#872\nBox: First box\nCategory: feature\nSpec: exact\nScope: module\nSurface: rust\nVerify: local-test\n";
    let worker = |status: &str| {
        let mut row = json!({"id": "w1", "fleet_name": "w1", "role": "worker", "status": status, "tier": "sonnet", "started": "2026-10-28T08:00:00Z", "eta_min": 30, "brief": brief});
        if status == "finished" {
            row.as_object_mut().unwrap().extend([("ended".into(), json!("2026-10-28T08:30:00Z")), ("actual_min".into(), json!(30)), ("outcome".into(), json!("not done")), ("report".into(), json!("Done: ticked one box."))]);
        }
        row
    };
    let issue = |body: &str| json!({"type": "issue_boxes", "item": "north#872", "body": body});
    let check = json!({"type": "done_check", "host": "h", "task_ref": "north#872", "head_sha": "abcdef1234567890", "command": "true", "exit": 0, "ran_ms": 1_793_175_000_000_i64});
    let outcome = || ledger.rows("SELECT outcome FROM runs WHERE agent = 'w1'");
    let read = |name: &str| vec![vec![Value::Text(name.to_owned())]];
    let first = |sha: &str| format!("- [x] First box. Evidence: `true` passed on main {sha} on 2026-10-28.\n");

    ledger.ingest(&[worker("running")]);
    Connection::open(&ledger.path).unwrap().execute("INSERT INTO events(ts, item, kind, actor, data) VALUES ('2026-10-28T08:01:00Z', 'north#872', 'claim', 'w1', '{\"box\": \"second-box\"}')", []).unwrap();
    ledger.ingest(&[check, issue(&format!("{}- [ ] Second box. Done when: `true` exits 0.\n", first("111111111111"))), worker("finished")]);
    assert_eq!(outcome(), read("done"), "its check passed, and the box ticked cites another commit");
    assert_eq!(ledger.rows("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'done_checks' AND name = 'done_checks_task_ref'"), read("done_checks_task_ref"), "the settle reads the checks of each run through the index, not a scan per run");
    ledger.ingest(&[issue(&format!("{}- [ ] Second box. Done when: `true` exits 0.\n", first("abcdef123456")))]);
    assert_eq!(outcome(), read("split"));
    ledger.ingest(&[issue(&format!("{}{}", first("abcdef123456"), "- [x] Second box. Evidence: `true` passed on main abcdef123456 on 2026-10-28.\n"))]);
    assert_eq!(outcome(), read("done"), "no box it claimed is left open");
}
