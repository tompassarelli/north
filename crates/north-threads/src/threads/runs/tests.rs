use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::{Value as Json, json};

use super::run;
use crate::threads::attribution::{Harness, losses};
use crate::threads::causes;
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

/// A long report's closing `Stop:` line fell outside the 600 characters the ledger keeps, so 40% of a day's worker
/// usage read as unattributed (north#706): the kept line names the worker's own cause, and a boundary's record wins.
/// Eleven of the 26 runs read as landing refusals over the 24 h to 2026-11-07T18:26Z ended `Stop: landing` on a queued
/// lane that then landed with the run's sha (north#829): a run whose lane landed is no harness loss.
#[test]
fn a_failed_run_is_attributed_by_its_kept_stop_line_and_a_boundary_record_overrides_its_report() {
    let ledger = ledger();
    let worker = |id: &str, report: &str, stop: Option<&str>| {
        json!({"id": id, "role": "worker", "fleet_name": format!("w-{id}"), "account": "claude-b", "usage_samples": [{"request": id, "ts": "2026-10-28T08:30:00Z", "weight": 100.0}],
               "tier": "high", "actual_min": 20, "started": "2026-10-28T08:20:00Z", "ended": "2026-10-28T08:40:00Z", "outcome": "not done", "report": report, "stop": stop})
    };
    let head: String = format!("Not done: {}", "the lane holds three files. ".repeat(30)).chars().take(600).collect();
    ledger.ingest(&[worker("long", &head, Some("Stop: landing")), worker("cut", "Not done: clippy is clean, so the lane goes next.", None), worker("told", "Not done: the proxy asked me to stop here.", None), worker("queued", "Not done: the lane is queued with the lander.", Some("Stop: landing"))]);
    ledger.ingest(&[json!({"push_join": 1, "fleet_name": "w-queued", "landed_sha": "abc1234", "landed_at": "2026-10-28T08:45:00.000Z"})]);
    let context = Context { database: &ledger.path, actor: "tester".into(), no_native: None, outside: &ledger.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
    for (session, cause) in [("w-cut", "halted"), ("w-told", "proxy")] {
        let argv: Vec<String> = ["cause", "record", "--session", session, "--source", "fleet", "--cause", cause].map(str::to_owned).to_vec();
        assert_eq!(causes::run(&argv, &context, &mut "".as_bytes(), &mut Vec::new(), &mut Vec::new()), 0);
    }
    let reading = losses(&Connection::open(&ledger.path).unwrap(), 0).unwrap();
    let classes: Vec<(Harness, usize)> = reading.losses.iter().map(|loss| (loss.harness, loss.runs)).collect();
    assert_eq!(classes, vec![(Harness::LandingRefusal, 1), (Harness::Halted, 1)]);
    assert_eq!(reading.proxy.0, 1, "a run the proxy cut short is the proxy's, apart from the harness classes");
    assert_eq!(reading.unclaimed_usd, 0.0, "{reading:?}");
}

/// north-730-d0235 opened `Blocked:` on ccx33 and the ready bar read it as Tom's on 2026-11-07, hiding a placement loss
/// (north#751). Workers now record a need with `north fleet need`, which writes a `host-need` cause row for their session:
/// a run whose session recorded one inside its window is placement whatever its report says, and a Blocked report on
/// Tom's decision stays his.
#[test]
fn a_run_whose_session_recorded_a_host_need_is_placement_and_a_blocked_decision_stays_toms() {
    let ledger = ledger();
    let worker = |id: &str, report: &str| {
        json!({"id": id, "role": "worker", "fleet_name": format!("w-{id}"), "account": "claude-b", "usage_samples": [{"request": id, "ts": "2026-10-28T08:30:00Z", "weight": 100.0}],
               "tier": "high", "actual_min": 20, "started": "2026-10-28T08:20:00Z", "ended": "2026-10-28T08:40:00Z", "outcome": "escalated", "report": report})
    };
    ledger.ingest(&[worker("need", "Blocked: I'm on ccx33, and the runner lane lives on gex45."), worker("tom", "Blocked: the pilot spends money.\nStop: decision")]);
    let context = Context { database: &ledger.path, actor: "tester".into(), no_native: None, outside: &ledger.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
    let argv: Vec<String> = ["cause", "record", "--session", "w-need", "--source", "host-need", "--cause", "placement", "--reason", "gex45: the runner lane lives there"].map(str::to_owned).to_vec();
    assert_eq!(causes::run(&argv, &context, &mut "".as_bytes(), &mut Vec::new(), &mut Vec::new()), 0);
    let reading = losses(&Connection::open(&ledger.path).unwrap(), 0).unwrap();
    let classes: Vec<(Harness, usize)> = reading.losses.iter().map(|loss| (loss.harness, loss.runs)).collect();
    assert_eq!(classes, vec![(Harness::Placement, 1)], "{reading:?}");
}

/// north-751-d0333 found runs that ended on the API's "Credit balance is too low" charged to the worker on 2026-11-07
/// when no boundary recorded a cause: a run the API refused for credit is a capacity loss.
#[test]
fn a_run_that_ends_on_the_api_credit_error_is_capacity() {
    let read = crate::threads::attribution::attribute("unclear", "Credit balance is too low", 0, None);
    assert_eq!(read, crate::threads::attribution::Cause::Harness(Harness::Capacity));
}

/// On 2026-11-07, 22 of the 24 Done runs from 23:00 to 06:14 UTC that expected a landing showed `landed = 0` on their row
/// though the push log named a landing by the same fleet session: a session on another host writes its row there, and the
/// lander host holds the landing (north#791). The lander host's push row gives the session's newest run before it the sha.
#[test]
fn a_run_ingested_on_another_host_takes_the_landing_the_lander_hosts_push_log_names_for_its_session() {
    let ledger = ledger();
    let run = |id: &str, started: &str, ended: &str| {
        json!({"id": id, "role": "worker", "fleet_name": "cpx62-n757-spool", "account": "claude-b", "usage_samples": [{"request": id, "ts": "2026-10-28T08:30:00Z", "weight": 100.0}],
               "tier": "high", "actual_min": 20, "started": started, "ended": ended, "outcome": "done", "report": "Done: queued.", "expects_landing": 1, "landed": 0})
    };
    ledger.ingest(&[run("first", "2026-10-28T06:00:00.000Z", "2026-10-28T06:20:00.000Z"), run("second", "2026-10-28T08:20:00.000Z", "2026-10-28T08:40:00.000Z")]);
    ledger.ingest(&[json!({"push_join": 1, "fleet_name": "cpx62-n757-spool", "landed_sha": "abc1234", "landed_at": "2026-10-28T08:45:00.000Z"})]);
    assert_eq!(
        ledger.rows("SELECT agent, landed, coalesce(landed_sha, '') FROM runs ORDER BY started"),
        vec![vec![Value::Text("first".into()), Value::Integer(0), Value::Text(String::new())], vec![Value::Text("second".into()), Value::Integer(1), Value::Text("abc1234".into())]]
    );
}

/// On 2026-11-07 from 06:52 UTC every whiterabbit worker-ledger batch carried about 2,050 push_join rows, each a scan of
/// the ledger's 10,090 runs, so one ingest held nexus's ingest lock 35 s, past the caller's 20 s ssh bound, and the spool
/// resent the same batch until nexus's CPU pressure starved every other ledger call.
#[test]
fn the_landing_join_probes_an_index_instead_of_scanning_the_runs() {
    let ledger = ledger();
    ledger.ingest(&[json!({"push_join": 1, "fleet_name": "cpx62-n757-spool", "landed_sha": "abc1234", "landed_at": "2026-10-28T08:45:00.000Z"})]);
    let db = Connection::open(&ledger.path).unwrap();
    let mut plan = db.prepare(&format!("EXPLAIN QUERY PLAN {}", super::record::PUSH_JOIN)).unwrap();
    let steps: Vec<String> = plan.query_map(rusqlite::params!["", "", ""], |row| row.get(3)).unwrap().map(Result::unwrap).collect();
    assert!(steps.iter().all(|step| !step.starts_with("SCAN")), "{steps:?}");
}

/// Two workers on two boxes of north#782 were one claim in the ledger, so the second ingest took the issue from the first.
#[test]
fn two_running_workers_on_two_boxes_of_one_issue_hold_one_claim_each_and_their_run_rows_name_the_issue_and_the_box() {
    let ledger = ledger();
    let worker = |id: &str, status: &str, box_text: Option<&str>| {
        let brief = format!("Item: makgora#782\n{}Category: feature\nSpec: exact\nScope: module\nSurface: rust\nVerify: local-test\n", box_text.map(|text| format!("Box: {text}\n")).unwrap_or_default());
        json!({"id": id, "role": "worker", "status": status, "tier": "sonnet", "started": "2026-10-28T08:00:00Z", "ended": "2026-10-28T08:30:00Z", "actual_min": 30, "outcome": "done", "report": "Done:", "eta_min": 60, "brief": brief})
    };
    ledger.ingest(&[worker("w1", "running", Some("the loss is classified")), worker("w2", "running", Some("the staffing replays"))]);
    assert_eq!(
        ledger.rows("SELECT item, owner FROM claims ORDER BY item"),
        [["makgora#782@the-loss-is-classified", "w1"], ["makgora#782@the-staffing-replays", "w2"]].map(|row| row.map(|text| Value::Text(text.to_owned())).to_vec())
    );
    ledger.ingest(&[worker("w1", "finished", Some("the loss is classified")), worker("w2", "finished", Some("the staffing replays"))]);
    assert!(ledger.rows("SELECT item FROM claims").is_empty());
    assert_eq!(
        ledger.rows("SELECT agent, item, box FROM runs WHERE item = 'makgora#782' ORDER BY agent"),
        [["w1", "makgora#782", "the loss is classified"], ["w2", "makgora#782", "the staffing replays"]].map(|row| row.map(|text| Value::Text(text.to_owned())).to_vec())
    );
}

/// north#798: the replica's capacity rows stopped on 2026-10-10 while admission kept deciding every build and lease; each
/// admission a host ships is one ledger row, a repeat adds none, and the ledger names each host's newest for the next shipment.
#[test]
fn a_shipped_capacity_admission_is_one_ledger_row_and_a_repeat_adds_none() {
    let ledger = ledger();
    let admission = |id: &str, host: &str, at: &str, decision: &str| json!({"type": "capacity", "id": id, "host": host, "at": at, "owner": "land-check", "class": "heavy", "decision": decision, "reason": "RUN", "queued_seconds": 0.0, "cpus": 4.0, "memory_mib": 8192.0});
    let batch = [admission("a", "nexus", "2026-10-28T08:00:00.000Z", "RESERVED"), admission("b", "nexus", "2026-10-28T08:30:00.000Z", "DEFER"), admission("c", "gex45", "2026-10-28T08:10:00.000Z", "REFUSED")];
    assert!(ledger.ingest(&batch).contains("3 capacity admissions"));
    assert!(!ledger.ingest(&batch).contains("capacity admissions"));
    assert_eq!(ledger.rows("SELECT count(*) FROM capacity_admissions"), [[Value::Integer(3)]]);
    let (code, seen, _) = ledger.call(&["run-ids", "--capacity"], "");
    assert_eq!((code, seen.as_str()), (0, "gex45\t2026-10-28T08:10:00.000Z\nnexus\t2026-10-28T08:30:00.000Z\n"));
}
