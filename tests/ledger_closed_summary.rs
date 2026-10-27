use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

fn stamp(hours_ago: i64) -> String {
    let output = Command::new("date").args(["-u", "-d", &format!("{hours_ago} hours ago"), "+%Y-%m-%dT%H:%M:%SZ"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[allow(clippy::too_many_arguments)]
fn insert(db: &Connection, agent: &str, item: Option<&str>, role: &str, tier: &str, category: &str, model: &str, hours_ago: i64, landed: i64) {
    db.execute(
        "INSERT INTO runs(agent, item, tier, category, started, ended, outcome, role, model, landed, input_tokens, cache_read_tokens, tokens) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?5, 'done', ?6, ?7, ?8, 3000000, 2000000, 100000)",
        rusqlite::params![agent, item, tier, category, stamp(hours_ago), role, model, landed],
    )
    .unwrap();
}

#[test]
fn ledger_summary_since_prints_usd_per_closed_for_each_tier_and_category_from_the_issues_github_closed() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let database = root.join("threads.db");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(
        "CREATE TABLE runs(agent TEXT PRIMARY KEY, item TEXT, tier TEXT, category TEXT, started TEXT, ended TEXT, outcome TEXT, role TEXT, model TEXT, \
         landed INTEGER, landed_sha TEXT, input_tokens INTEGER, cache_read_tokens INTEGER, tokens INTEGER);
         CREATE TABLE items(ns TEXT, number INTEGER, state TEXT, closed TEXT);",
    )
    .unwrap();
    insert(&db, "a1", Some("north#1"), "worker", "medium", "feature", "claude-opus-5-5", 30, 0);
    insert(&db, "a2", Some("north#1"), "worker", "medium", "feature", "claude-opus-5-5", 5, 1);
    insert(&db, "b1", Some("north#2"), "worker", "sonnet", "tooling", "claude-sonnet-5-5", 4, 1);
    insert(&db, "b2", Some("north#3"), "worker", "sonnet", "tooling", "claude-sonnet-5-5", 3, 1);
    insert(&db, "c1", Some("north#4"), "worker", "haiku", "mechanical", "claude-haiku-5-5", 3, 1);
    insert(&db, "p1", None, "proxy", "claude-opus-5-5", "unknown", "claude-opus-5-5", 6, 0);
    drop(db);

    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    let closed = format!(r#"[{{"number":1,"closedAt":"{0}"}},{{"number":2,"closedAt":"{0}"}},{{"number":3,"closedAt":"{0}"}}]"#, stamp(1));
    fs::write(&gh, format!("#!/bin/sh\nprintf '%s' '{closed}'\n")).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["ledger", "summary", "--since", "24h", "--json"])
        .env("THREADS_DB", &database)
        .env("HOME", root)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let cell = |tier: &str, category: &str| {
        report["cells"].as_array().unwrap().iter().find(|cell| cell["tier"] == tier && cell["category"] == category).unwrap().clone()
    };
    let near = |value: &Value, expected: f64| (value.as_f64().unwrap() - expected).abs() < 1e-6;
    assert_eq!(report["closed"], 3);
    assert_eq!(report["cells"].as_array().unwrap().len(), 2);
    assert!(near(&cell("opus-medium", "feature")["usd_per_closed"], 10.4 * 2.0));
    assert_eq!(cell("sonnet", "tooling")["closed"], 2);
    assert!(near(&cell("sonnet", "tooling")["usd_per_closed"], 5.2));
    let spend = 10.4 + 5.2 * 2.0 + 0.27 + 10.4;
    assert!(near(&report["all_usd_per_closed"], spend / 3.0), "{report}");
    assert!(near(&report["leads_usd_per_closed"], 10.4 / 3.0));
}
