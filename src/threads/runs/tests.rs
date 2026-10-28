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

/// wisp's issues carry labels, smashcraft's listing is unreadable, any other repo has none.
fn github(joined: &str) -> Option<Result<String, String>> {
    let reply = if joined.contains("-R tompassarelli/wisp") {
        json!([{"number": 3, "labels": [{"name": "bug"}]}, {"number": 9, "labels": [{"name": "enhancement"}, {"name": "theme:balance"}]}]).to_string()
    } else if joined.contains("-R tompassarelli/smashcraft") {
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
    fn row(&self, query: &str) -> String {
        let db = Connection::open(&self.path).unwrap();
        let mut statement = db.prepare(query).unwrap();
        let width = statement.column_count();
        statement
            .query_row([], |row| {
                Ok((0..width)
                    .map(|index| match row.get::<_, Value>(index).unwrap() {
                        Value::Null => String::new(),
                        Value::Integer(number) => number.to_string(),
                        Value::Real(number) => number.to_string(),
                        Value::Text(text) => text,
                        Value::Blob(_) => "<blob>".into(),
                    })
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
    }

    fn rows(&self, query: &str) -> Vec<Vec<Value>> {
        let db = Connection::open(&self.path).unwrap();
        let mut statement = db.prepare(query).unwrap();
        let width = statement.column_count();
        statement.query_map([], |row| (0..width).map(|index| row.get::<_, Value>(index)).collect()).unwrap().collect::<Result<_, _>>().unwrap()
    }

    fn run_ids(&self, settled: bool) -> Vec<String> {
        let (code, out, err) = self.call(if settled { &["run-ids", "--settled"] } else { &["run-ids"] }, "");
        assert_eq!(code, 0, "{err}");
        out.lines().map(str::to_owned).collect()
    }
}

const CREATION: &str = "Item: smashcraft#4\nCategory: tooling\nSpec: measured\nScope: module\nSurface: shell";

fn creation() -> Json {
    json!({"id": "a333", "status": "running", "model": "gpt-6.1-sol", "effort": "medium", "tier": "medium", "started": "2026-10-08T03:00:00.511000Z", "staffed": "2026-10-08T03:00:00.511000Z",
           "eta_min": 30, "brief": CREATION, "brief_text": CREATION, "brief_origin": "codex.PostToolUse.spawn_agent", "brief_path": "/durable/original.txt", "brief_sha256": "exact-original-hash"})
}

fn collected(model: Option<(&str, &str)>, ended: &str, minutes: u32, outcome: &str) -> Json {
    let mut row = json!({"id": "a333", "status": "finished", "started": "2026-10-08T03:00:00.512Z", "ended": ended, "tier": "medium", "actual_min": minutes, "outcome": outcome});
    if let Some((model, effort)) = model {
        row["model"] = json!(model);
        row["effort"] = json!(effort);
    }
    row
}

#[test]
fn a_native_creation_row_keeps_its_brief_model_and_effort_while_running_and_holds_its_item_unsettled() {
    let ledger = ledger();
    ledger.ingest(&[creation()]);
    assert_eq!(ledger.row("SELECT outcome, category, verify, model, effort FROM runs WHERE agent = 'a333'"), "running|tooling||gpt-6.1-sol|medium");
    assert_eq!(ledger.row("SELECT item, owner, since, eta_min FROM claims"), "smashcraft#4|a333|2026-10-08T03:00:00.511000Z|30");
    assert!(!ledger.run_ids(true).contains(&"a333".to_owned()));
}

#[test]
fn a_creation_row_keeps_observed_model_effort_and_original_brief_through_replacement_completion_and_backfill() {
    let ledger = ledger();
    let model = |ledger: &Ledger| ledger.row("SELECT model, effort FROM runs WHERE agent = 'a333'");
    ledger.ingest(&[creation()]);
    ledger.ingest(&[collected(None, "2026-10-08T03:10:00Z", 10, "running")]);
    assert_eq!(model(&ledger), "gpt-6.1-sol|medium", "a partial replacement keeps the observed model and effort");
    ledger.ingest(&[collected(Some(("gpt-6-sol", "high")), "2026-10-08T03:15:00Z", 15, "running")]);
    assert_eq!(model(&ledger), "gpt-6-sol|high", "a partial replacement takes an explicit model and effort");
    let mut done = collected(None, "2026-10-08T03:20:00Z", 20, "done");
    done["staffed"] = json!("2026-10-08T02:59:59Z");
    done["brief"] = json!("Item: smashcraft#4\nVerify: native");
    ledger.ingest(&[done]);
    assert_eq!(model(&ledger), "gpt-6-sol|high", "completion keeps the latest observed model and effort");
    let promoted = ledger.row("SELECT outcome, category, verify, brief_origin, brief_path, brief_sha256, started, staffed FROM runs WHERE agent = 'a333'");
    assert_eq!(promoted, "done|tooling||codex.PostToolUse.spawn_agent|/durable/original.txt|exact-original-hash|2026-10-08T03:00:00.511000Z|2026-10-08T02:59:59Z");
    assert!(ledger.run_ids(true).contains(&"a333".to_owned()), "a finished worker run is settled");
    let backfill = |extra: Json| {
        let mut row = json!({"id": "a333", "backfill": true, "status": "finished", "ended": "2026-10-08T03:20:00Z", "tier": "medium", "actual_min": 20, "outcome": "done"});
        row.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        row
    };
    ledger.ingest(&[backfill(json!({"brief": "Category: feature\nSpec: exact\nScope: cross-repo\nSurface: ts\nVerify: native"}))]);
    assert_eq!(ledger.row("SELECT category, spec, scope, surface, verify FROM runs WHERE agent = 'a333'"), "tooling|measured|module|shell|", "a backfill keeps the original facets, an absent one too");
    assert_eq!(model(&ledger), "gpt-6-sol|high", "a backfill naming no model keeps the observed one");
    ledger.ingest(&[backfill(json!({"model": "gpt-6.1-sol", "effort": "low"})), backfill(json!({"model": null, "effort": null}))]);
    assert_eq!(model(&ledger), "gpt-6.1-sol|low", "an explicit backfill wins and a later null keeps it");
}

#[test]
fn an_unknown_native_creation_takes_the_collector_started_and_a_claude_creation_does_too() {
    let ledger = ledger();
    ledger.ingest(&[json!({"id": "a333-unknown", "status": "running", "tier": "gpt-6.1-sol medium", "brief_text": "Goal: unknown observations", "brief_origin": "codex.PostToolUse.spawn_agent"})]);
    ledger.ingest(&[json!({"id": "a333-unknown", "status": "finished", "started": "2026-10-08T03:00:00.512Z", "ended": "2026-10-08T03:20:00Z", "tier": "gpt-6.1-sol medium", "actual_min": 20, "outcome": "running"})]);
    ledger.ingest(&[json!({"id": "a333-unknown", "backfill": true, "status": "finished", "ended": "2026-10-08T03:20:00Z", "tier": "gpt-6.1-sol medium", "actual_min": 20, "outcome": "running"})]);
    assert_eq!(ledger.row("SELECT started, model IS NULL, effort IS NULL FROM runs WHERE agent = 'a333-unknown'"), "2026-10-08T03:00:00.512Z|1|1");
    ledger.ingest(&[json!({"id": "a333-claude", "status": "running", "tier": "medium", "started": "2026-10-08T03:00:00.511000Z", "brief_text": "Goal: Claude creation", "brief_origin": "claude.PostToolUse.Agent"})]);
    ledger.ingest(&[json!({"id": "a333-claude", "status": "finished", "tier": "medium", "started": "2026-10-08T03:00:00.512Z", "ended": "2026-10-08T03:20:00Z", "actual_min": 20, "outcome": "running"})]);
    assert_eq!(ledger.row("SELECT started FROM runs WHERE agent = 'a333-claude'"), "2026-10-08T03:00:00.512Z");
}

#[test]
fn a_backfill_fills_attribution_and_eta_while_a_nonworker_report_follows_supplied_evidence() {
    let ledger = ledger();
    ledger.ingest(&[json!({"id": "attribution", "landed": 1, "landed_sha": "a".repeat(40), "ended": "2026-10-08T06:40:00Z", "tier": "medium", "actual_min": 25, "outcome": "done", "report": "Done: attributed work."})]);
    let err = ledger.ingest(&[json!({"id": "attribution", "backfill": true, "landed": 0, "item": "wisp#71", "category": "tooling", "eta_min": 25, "ended": "2026-10-08T06:40:00Z", "tier": "medium", "actual_min": 25, "outcome": "not done", "report": "Not done: later worker evidence."})]);
    assert!(err.starts_with("NorthThreads: 0 runs recorded, 1 backfilled in "), "{err}");
    assert_eq!(ledger.row("SELECT item, category, eta_min, landed, landed_sha, outcome, report FROM runs WHERE agent = 'attribution'"), format!("wisp#71|tooling|25|1|{}|done|Done: attributed work.", "a".repeat(40)));
    let parent = |extra: Json| {
        let mut row = json!({"id": "nonworker-attribution", "role": "parent"});
        row.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        row
    };
    ledger.ingest(&[parent(json!({"ended": "2026-10-08T06:40:00Z", "tier": "medium", "actual_min": 25, "outcome": "unclear", "report": "Running: initial evidence.", "category": "unknown"}))]);
    ledger.ingest(&[parent(json!({"ended": "2026-10-08T07:00:00Z", "outcome": "done", "report": "Done: current explicit evidence.", "category": "tooling", "compactions": 2}))]);
    let shown = "SELECT ended, category, role, compactions, outcome, report FROM runs WHERE agent = 'nonworker-attribution'";
    assert_eq!(ledger.row(shown), "2026-10-08T07:00:00Z|tooling|parent|2|done|Done: current explicit evidence.");
    ledger.ingest(&[parent(json!({"ended": "2026-10-08T07:20:00Z"}))]);
    assert_eq!(ledger.row(shown), "2026-10-08T07:20:00Z|tooling|parent|2|done|Done: current explicit evidence.");
}

#[test]
fn a_worker_taking_a_held_item_is_a_handoff_finishing_releases_it_and_a_later_run_follows_the_last() {
    let ledger = ledger();
    let worker = |id: &str, status: &str, brief: &str, eta: Json| json!({"id": id, "status": status, "tier": "medium", "started": "2026-10-09T01:00:00Z", "ended": "2026-10-09T01:30:00Z", "actual_min": 20, "eta_min": eta, "outcome": "done", "brief": brief});
    ledger.ingest(&[worker("h1", "running", "Item: north#7", json!(2.5))]);
    assert_eq!(ledger.row("SELECT owner, eta_min FROM claims WHERE item = 'north#7'"), "h1|2", "an ETA rounds half to even");
    ledger.ingest(&[worker("h2", "running", "Item: north#7 Follows: h1", json!(3.5))]);
    assert_eq!(ledger.row("SELECT owner, eta_min FROM claims WHERE item = 'north#7'"), "h2|4");
    ledger.ingest(&[worker("h2", "finished", "Item: north#7", json!(3.5)), worker("h4", "finished", "Item: north#7", json!(null))]);
    let events: Vec<String> = ledger.rows("SELECT kind, actor, tier, data FROM events ORDER BY seq").into_iter().map(|row| format!("{row:?}")).collect();
    assert_eq!(
        events,
        [
            r#"[Text("claim"), Text("h1"), Text("medium"), Text("{\"eta_min\": 2.5}")]"#,
            r#"[Text("handoff"), Text("h2"), Text("medium"), Text("{\"from\": \"h1\", \"to\": \"h2\"}")]"#,
            r#"[Text("release"), Text("h2"), Text("medium"), Text("{\"why\": \"finished\"}")]"#,
            r#"[Text("handoff"), Text("h4"), Text("medium"), Text("{\"from\": \"h2\", \"to\": \"h4\"}")]"#,
        ]
    );
    assert_eq!(ledger.row("SELECT count(*) FROM claims"), "0");
}

#[test]
fn an_unlabeled_run_takes_its_category_from_its_issue_labels_with_one_listing_per_repo() {
    let ledger = ledger();
    let done = |id: &str, extra: Json| {
        let mut row = json!({"id": id, "ended": "2026-10-09T00:00:00Z", "tier": "medium", "actual_min": 5, "outcome": "done"});
        row.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        row
    };
    ledger.ingest(&[
        done("u1", json!({"brief": "Item: wisp#3"})),
        done("u2", json!({"refs": ["nope", "wisp#9"]})),
        done("u3", json!({"brief": "Item: smashcraft#8"})),
        done("u4", json!({"brief": "Item: ops#1"})),
        done("u5", json!({"brief": "Item: wisp#3 Category: research"})),
    ]);
    let categories: Vec<Vec<Value>> = ledger.rows("SELECT agent, category FROM runs ORDER BY agent");
    let text = |value: &str| Value::Text(value.into());
    assert_eq!(categories, [["u1", "bug-known-cause"], ["u2", "balance-tuning"], ["u3", "unknown"], ["u4", "unknown"], ["u5", "research"]].map(|row| row.map(text).to_vec()));
    let listed: Vec<String> = ledger.outside.calls.borrow().iter().map(|call| call[3].clone()).collect();
    assert_eq!(listed, ["tompassarelli/wisp", "tompassarelli/smashcraft"]);
}

#[test]
fn an_unknown_role_or_an_unreadable_row_refuses_the_whole_batch() {
    let ledger = ledger();
    let fine = json!({"id": "x1", "ended": "2026-10-09T00:00:00Z", "tier": "medium", "actual_min": 5, "outcome": "done"});
    let (code, _, err) = ledger.call(&["ingest"], &format!("{fine}\n{}\n", json!({"id": "x2", "role": "boss"})));
    assert_eq!((code, err.as_str()), (1, "NorthThreads: unknown run role 'boss'\n"));
    let (code, _, _) = ledger.call(&["ingest"], &format!("{fine}\nnot json\n"));
    assert_eq!(code, 1);
    assert!(ledger.run_ids(false).is_empty());
}

#[test]
fn a_row_follows_stands_when_its_brief_names_none_even_when_empty() {
    let ledger = ledger();
    let run = |id: &str, follows: Json, brief: &str| json!({"id": id, "ended": "2026-10-09T00:00:00Z", "tier": "medium", "actual_min": 5, "outcome": "done", "follows": follows, "brief": brief});
    ledger.ingest(&[run("empty", json!(""), "Category: tooling"), run("named", json!("row"), "Follows: brief"), run("unnamed", json!("row"), "Category: tooling")]);
    assert_eq!(ledger.rows("SELECT agent, follows FROM runs ORDER BY agent"), [["empty", ""], ["named", "brief"], ["unnamed", "row"]].map(|row| row.map(|cell| Value::Text(cell.into())).to_vec()));
}

#[test]
fn run_ids_settled_leaves_out_running_and_recent_unlanded_runs_that_expect_to_land() {
    let ledger = ledger();
    let done = |id: &str, ended: &str, extra: Json| {
        let mut row = json!({"id": id, "ended": ended, "tier": "medium", "actual_min": 5, "outcome": "done", "expects_landing": 1});
        row.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        row
    };
    ledger.ingest(&[
        done("recent", "2026-10-27T00:00:00Z", json!({})),
        done("landed", "2026-10-27T00:00:00Z", json!({"landed_at": "2026-10-27T01:00:00Z"})),
        done("old", "2026-10-20T00:00:00Z", json!({})),
        done("parent", "2026-10-27T00:00:00Z", json!({"role": "parent"})),
    ]);
    assert_eq!(ledger.run_ids(false), ["landed", "old", "parent", "recent"]);
    assert_eq!(ledger.run_ids(true), ["landed", "old"]);
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
fn a_window_rise_is_priced_and_split_across_the_runs_that_sent_requests_during_it() {
    let first = ledger();
    let err = first.ingest(&first_batch());
    assert!(err.ends_with("; token weight for models without a list price: alpha-2, zeta-1\n"), "{err}");
    assert_eq!(first.row("SELECT cost_usd FROM runs WHERE agent = 'a'"), "2");
    let ledger = ledger();
    ledger.ingest(&[first_batch(), second_batch()].concat());
    let costs = ledger.rows("SELECT agent, cost_usd FROM runs ORDER BY agent");
    let usd = |row: &Vec<Value>| match row[1] {
        Value::Real(usd) => usd,
        _ => panic!("{row:?}"),
    };
    let expected = [2.0 * 730.0 / 1000.0, 2.0 * 270.0 / 1000.0 + 3.0 + 1.5, 3.0];
    for (row, expected) in costs.iter().zip(expected) {
        assert!((usd(row) - expected).abs() < 1e-9, "{costs:?}");
    }
}

fn count_writes(ledger: &Ledger) {
    let db = Connection::open(&ledger.path).unwrap();
    db.execute_batch("CREATE TABLE writes(n INTEGER NOT NULL); INSERT INTO writes VALUES (0);").unwrap();
    for table in ["runs", "run_costs"] {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            db.execute_batch(&format!("CREATE TRIGGER count_{table}_{event} AFTER {event} ON {table} BEGIN UPDATE writes SET n = n + 1; END;")).unwrap();
        }
    }
}

#[test]
fn an_ingest_of_unchanged_sources_writes_no_runs_or_run_costs_rows() {
    let ledger = ledger();
    ledger.ingest(&[first_batch(), second_batch()].concat());
    assert!(ledger.rows("SELECT * FROM run_costs").len() >= 5);
    count_writes(&ledger);
    ledger.ingest(&[]);
    assert_eq!(ledger.row("SELECT n FROM writes"), "0", "a second ingest of unchanged sources rewrote runs or run_costs rows");
    ledger.ingest(&[readout("claude", "b", 40, 23.0)]);
    assert_eq!(ledger.row("SELECT n FROM writes"), "0", "a flat readout rewrote runs or run_costs rows");
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
