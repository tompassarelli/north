use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::{Value as Json, json};

const COSTS: &str = "[[service]]\nid = \"claude-b\"\nusd_per_week = 50.0\n\n[[service]]\nid = \"codex\"\nusd_per_week = 100.0\n";

fn ingest(root: &Path, rows: &[Json]) {
    let mut child = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"))
        .arg("ingest")
        .env("HOME", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("AGENTS_OPERATOR_DIR", root)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input: String = rows.iter().map(|row| format!("{row}\n")).collect();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn ledger() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    std::fs::write(scratch.path().join("costs.toml"), COSTS).unwrap();
    scratch
}

fn readout(provider: &str, account: &str, minute: u32, used: f64) -> Json {
    json!({"type": "usage", "ts": format!("2026-10-11T00:{minute:02}:00Z"), "provider": provider, "account": account,
           "kind": "window", "name": if provider == "codex" { "week" } else { "weekly_all" }, "used_pct": used,
           "resets_at": "2026-10-15T00:00:00Z"})
}

fn run(id: &str, account: &str, samples: &[(u32, f64)]) -> Json {
    let samples: Vec<Json> = samples
        .iter()
        .map(|(minute, weight)| json!({"request": format!("{id}-{minute}"), "ts": format!("2026-10-11T00:{minute:02}:00Z"), "weight": weight}))
        .collect();
    json!({"id": id, "role": "routine", "account": account, "usage_samples": samples, "tier": "high", "actual_min": 20,
           "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:20:00Z", "outcome": "done"})
}

fn first_batch() -> Vec<Json> {
    vec![
        readout("claude", "b", 0, 10.0),
        readout("claude", "b", 10, 14.0),
        readout("claude", "b", 20, 20.0),
        readout("codex", "", 0, 10.0),
        readout("codex", "", 10, 13.0),
        run("a", "claude-b", &[(5, 730.0)]),
    ]
}

fn second_batch() -> Vec<Json> {
    vec![
        readout("claude", "b", 30, 23.0),
        run("b", "claude-b", &[(8, 270.0), (15, 100.0), (25, 30.0)]),
        run("c", "codex", &[(5, 400.0)]),
    ]
}

fn rows(root: &Path, sql: &str) -> Vec<Vec<Value>> {
    let db = Connection::open(root.join("threads.db")).unwrap();
    let mut statement = db.prepare(sql).unwrap();
    let width = statement.column_count();
    statement
        .query_map([], |row| (0..width).map(|i| row.get::<_, Value>(i)).collect())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn count_writes(root: &Path) {
    let db = Connection::open(root.join("threads.db")).unwrap();
    db.execute_batch("CREATE TABLE writes(n INTEGER NOT NULL); INSERT INTO writes VALUES (0);").unwrap();
    for table in ["runs", "run_costs"] {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            db.execute_batch(&format!(
                "CREATE TRIGGER count_{table}_{event} AFTER {event} ON {table} BEGIN UPDATE writes SET n = n + 1; END;"
            ))
            .unwrap();
        }
    }
}

fn writes(root: &Path) -> i64 {
    let db = Connection::open(root.join("threads.db")).unwrap();
    db.query_row("SELECT n FROM writes", [], |row| row.get(0)).unwrap()
}

#[test]
fn threads_ingest_of_unchanged_sources_writes_no_runs_or_run_costs_rows() {
    let scratch = ledger();
    let root = scratch.path();
    ingest(root, &[first_batch(), second_batch()].concat());
    assert!(rows(root, "SELECT * FROM run_costs").len() >= 5);
    count_writes(root);
    ingest(root, &[]);
    assert_eq!(writes(root), 0, "a second ingest of unchanged sources rewrote runs or run_costs rows");
    ingest(root, &[readout("claude", "b", 40, 23.0)]);
    assert_eq!(writes(root), 0, "a flat readout rewrote runs or run_costs rows");
}

#[test]
fn threads_incremental_ingest_leaves_runs_and_run_costs_as_a_full_ingest_does() {
    let full = ledger();
    ingest(full.path(), &[first_batch(), second_batch()].concat());
    let incremental = ledger();
    ingest(incremental.path(), &first_batch());
    let untracked = rows(incremental.path(), "SELECT * FROM run_costs WHERE agent = '(untracked)'");
    assert_eq!(untracked.len(), 1, "the codex step starts unallocated");
    ingest(incremental.path(), &second_batch());
    ingest(incremental.path(), &[]);
    for sql in ["SELECT * FROM run_costs ORDER BY account, started, ended, agent", "SELECT * FROM runs ORDER BY agent"] {
        let expected = rows(full.path(), sql);
        assert!(!expected.is_empty(), "{sql}");
        assert_eq!(rows(incremental.path(), sql), expected, "{sql}");
    }
    let costs = rows(full.path(), "SELECT agent, cost_usd FROM runs ORDER BY agent");
    assert!(costs.iter().all(|row| matches!(row[1], Value::Real(usd) if usd > 0.0)), "{costs:?}");
}
