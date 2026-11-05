use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;
use crate::ledger::closed::SpendRun;
use crate::threads::calibration::rework::{Commit, Judged, Landing, judge};

pub const NOW: i64 = 1_793_793_600;

pub fn at(text: &str) -> i64 {
    Timestamp::parse(text).unwrap().unix_seconds()
}

pub fn ledger() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    db.execute_batch(crate::threads::landing::SCHEMA).unwrap();
    db.execute_batch(include_str!("fixtures/ledger.sql")).unwrap();
    db
}

fn commit(sha: &str, when: &str, subject: &str, files: &[&str]) -> Commit {
    Commit { sha: sha.repeat(20), at: at(when), subject: subject.into(), body: String::new(), files: files.iter().map(|file| (*file).to_owned()).collect() }
}

pub fn history() -> Vec<(String, Vec<Commit>)> {
    let commits = vec![
        commit("f6", "2026-10-25T14:30:00Z", "Add the old thing", &["src/old.rs"]),
        commit("a1", "2026-11-01T10:30:00Z", "Add widget", &["src/widget.rs"]),
        commit("b2", "2026-11-01T12:30:00Z", "Add gadget", &["src/gadget.rs"]),
        commit("c3", "2026-11-01T14:30:00Z", "Add gizmo", &["src/gizmo.rs"]),
        commit("99", "2026-11-01T20:00:00Z", "Fix the widget", &["src/widget.rs"]),
        commit("88", "2026-11-02T00:00:00Z", "Revert \"Add gadget\"", &["src/gadget.rs"]),
        commit("d4", "2026-11-03T14:20:00Z", "Add doohickey", &["src/doohickey.rs"]),
    ];
    vec![("north".to_owned(), commits)]
}

pub fn judged(db: &Connection) -> BTreeMap<String, Judged> {
    let landings: Vec<Landing> = db
        .prepare("SELECT agent, item, landed_sha FROM runs WHERE landed_sha IS NOT NULL")
        .unwrap()
        .query_map([], |row| Ok(Landing { agent: row.get(0)?, item: row.get(1)?, sha: row.get(2)? }))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    judge(&landings, &history(), &BTreeSet::new())
}

pub fn spend_runs(db: &Connection, judged: &BTreeMap<String, Judged>) -> Vec<SpendRun> {
    db.prepare("SELECT agent, item, tier, category, coalesce(arm, ''), model, started, ended, landed, input_tokens, cache_read_tokens, tokens FROM runs ORDER BY agent")
        .unwrap()
        .query_map([], |row| {
            let agent: String = row.get(0)?;
            Ok(SpendRun {
                agent: agent.clone(),
                item: row.get(1)?,
                role: "worker".into(),
                tier: row.get(2)?,
                category: row.get(3)?,
                arm: row.get(4)?,
                model: row.get(5)?,
                started: row.get(6)?,
                ended: row.get(7)?,
                landed: row.get::<_, i64>(8)? == 1,
                reworked: judged.get(&agent).is_some_and(|judged| judged.cause.is_some()),
                input: row.get(9)?,
                cache_read: row.get(10)?,
                output: row.get(11)?,
                skills: Vec::new(),
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
