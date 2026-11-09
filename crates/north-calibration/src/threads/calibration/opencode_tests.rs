use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

const NOW: &str = "2026-11-10T12:00:00Z";

#[derive(Default)]
struct Seeded {
    agent: &'static str,
    item: &'static str,
    model: &'static str,
    provider: &'static str,
    category: &'static str,
    started: &'static str,
    landed: i64,
    sha: &'static str,
    staffed: Option<&'static str>,
    follows: Option<&'static str>,
    cost: f64,
}

fn ledger() -> Connection {
    let db = Connection::open_in_memory().expect("memory ledger");
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).expect("schema");
    db.execute_batch(
        "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT;
         CREATE TABLE done_checks(host TEXT, task_ref TEXT, head_sha TEXT, command TEXT, exit INTEGER, ran_ms INTEGER, PRIMARY KEY(host, ran_ms, head_sha, command)) STRICT;",
    )
    .expect("window and check tables");
    db
}

fn run(db: &Connection, seed: Seeded) {
    db.execute(
        "INSERT INTO runs(agent, item, follows, tier, category, started, ended, minutes, outcome, landed, landed_sha, role, model, provider, staffed, cost_usd)
         VALUES (?1, ?2, ?3, 'sonnet', ?4, ?5, ?5, 60, 'done', ?6, ?7, 'worker', ?8, ?9, ?10, ?11)",
        rusqlite::params![seed.agent, seed.item, seed.follows, seed.category, seed.started, seed.landed, seed.sha, seed.model, seed.provider, seed.staffed, seed.cost],
    )
    .expect("run row");
}

fn usage(db: &Connection, ts: &str, name: &str, used_pct: f64) {
    db.execute("INSERT INTO usage(ts, provider, account, kind, name, used_pct) VALUES (?1, 'opencode', 'go', 'window', ?2, ?3)", rusqlite::params![ts, name, used_pct]).expect("window reading");
}

fn cells<'a>(text: &'a str, week: &str, model: &str) -> Vec<&'a str> {
    text.lines()
        .find(|line| line.starts_with(week) && line.split_whitespace().nth(1) == Some(model))
        .unwrap_or_else(|| panic!("no {week} {model} row in:\n{text}"))
        .split_whitespace()
        .collect()
}

#[test]
fn a_week_shows_the_paid_and_free_flash_rows_with_their_landing_escalation_go_window_and_sonnet_cost() {
    let db = ledger();
    run(&db, Seeded { agent: "ds-go-1", item: "north#1", model: "opencode-go/deepseek-v4.1-flash", provider: "opencode", category: "bug-known-cause", started: "2026-11-04T08:00:00Z", landed: 1, sha: "abc123", staffed: Some("2026-11-04T07:30:00Z"), ..Seeded::default() });
    run(&db, Seeded { agent: "ds-free-1", item: "north#2", model: "opencode/deepseek-v4.1-flash-free", provider: "opencode", category: "mechanical", started: "2026-11-05T10:00:00Z", ..Seeded::default() });
    run(&db, Seeded { agent: "claude-1", item: "north#1", model: "claude-sonnet-5-5", provider: "claude", category: "unknown", started: "2026-11-04T10:00:00Z", landed: 1, sha: "def456", follows: Some("ds-go-1"), cost: 1.25, ..Seeded::default() });
    run(&db, Seeded { agent: "claude-2", item: "north#7", model: "claude-sonnet-5-5", provider: "claude", category: "bug-known-cause", started: "2026-11-04T11:00:00Z", landed: 1, sha: "fed321", cost: 2.5, ..Seeded::default() });
    db.execute("INSERT INTO done_checks(host, task_ref, head_sha, command, exit, ran_ms) VALUES ('h', 'north#1', 'abc123', 'cmd', 0, 1)", []).expect("passing check");
    for (ts, name, used) in [("2026-11-04T06:00:00Z", "5h", 41.0), ("2026-11-04T07:45:00Z", "5h", 99.8), ("2026-11-04T06:00:00Z", "week", 18.0), ("2026-11-04T06:00:00Z", "month", 7.0)] {
        usage(&db, ts, name, used);
    }
    let text = super::rows_text(&db, "7d", Timestamp::parse(NOW).expect("now")).expect("report");
    let go = cells(&text, "2026-11-02", "go-flash");
    assert_eq!(go[2..].to_vec(), vec!["1", "1", "1", "1", "1.25", "2.50", "99.8%", "18.0%", "7.0%", "0.5"], "the paid row:\n{text}");
    let free = cells(&text, "2026-11-02", "free-flash");
    assert_eq!(free[2..].to_vec(), vec!["1", "0", "0", "0", "0.00", "-", "-", "-", "-", "-"], "the free row:\n{text}");
}
