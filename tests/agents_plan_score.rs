use std::fs;
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

const GH: &str = "#!/bin/sh\nif [ \"$2\" = list ]; then printf '%s\\n' '[{\"number\":9,\"state\":\"OPEN\",\"closedAt\":null,\"body\":\"- [ ] unrelated backlog\"}]'; exit; fi\nprintf '%s\\n' '{\"state\":\"CLOSED\",\"closedAt\":\"2020-01-07T00:00:00Z\",\"body\":\"- [x] verified\"}'\n";

#[test]
fn a_passed_target_scores_three_verdicts_one_row_and_the_budget_from_the_usage_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let operator = root.path().join("operator");
    let tools = root.path().join("bin");
    fs::create_dir_all(&operator).unwrap();
    fs::create_dir_all(&tools).unwrap();
    fs::write(operator.join("orchestration.toml"), "[projects.north]\ntarget = \"2020-01-08T00:00:00Z\"\nshare = 50\ngoal = [\"north#123\"]\n").unwrap();
    fs::write(operator.join("costs.toml"), "[[service]]\nid = \"build\"\nkind = \"cloud\"\nusd_per_week = 12.5\n[[service]]\nid = \"gemini\"\nkind = \"ai-plan\"\nusd_per_week = 50\n").unwrap();
    let db = Connection::open(root.path().join("snapshot.db")).unwrap();
    let now = north_core::foundation::clock::Timestamp::now();
    let later = north_core::foundation::clock::Timestamp::from_unix_seconds(now.unix_seconds() + 3 * 86400);
    db.execute_batch(
        "CREATE TABLE runs(item TEXT, ended TEXT, cost_usd REAL); INSERT INTO runs VALUES('north#123', '2020-01-07T00:00:00Z', 2.5);
         CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT;",
    )
    .unwrap();
    db.execute(
        "INSERT INTO usage VALUES(?1, 'gemini', 'gemini', 'window', 'week', 20, 10080, ?2, NULL, 'fixture')",
        [now.iso(), later.iso()],
    )
    .unwrap();
    drop(db);
    let gh = tools.join("gh");
    north_core::foundation::shim::write_executable(&gh, GH);
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["agents", "plan", "--score", "north"])
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("HOME", root.path())
        .env("AGENTS_OPERATOR_DIR", &operator)
        .env("THREADS_DB", root.path().join("snapshot.db"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("PATH", format!("{}:{}", tools.display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().collect::<Vec<_>>(), ["north on-date: pass", "north in-budget: pass", "north spec-pass: pass"]);
    let written = fs::read_to_string(root.path().join("state/agents/goal-scores.jsonl")).unwrap();
    let rows: Vec<&str> = written.lines().collect();
    assert_eq!(rows.len(), 1);
    let row: Value = serde_json::from_str(rows[0]).unwrap();
    assert_eq!(row["spent_usd"].as_f64(), Some(2.5));
    assert_eq!(row["budget_usd"].as_f64(), Some(20.0));
}
