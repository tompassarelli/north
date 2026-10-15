use rusqlite::Connection;

use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::gates::{Answerer, Closure, GateBook, GateRequest};
use crate::fleet::turns::{RunRecord, RunSink, Tokens, Turn, TurnExit};

use super::Ledger;

fn ledger(directory: &tempfile::TempDir) -> Ledger {
    Ledger::open(&directory.path().join("ledger.db")).unwrap()
}

fn request(session: &str, default_after_ms: u64) -> GateRequest {
    GateRequest {
        session: session.into(),
        question: "Rent?".into(),
        options: vec!["yes".into(), "no".into()],
        recommend: 1,
        default: 2,
        default_after_ms,
    }
}

fn run(session: &str, model: &str, input: u64, output: u64) -> RunRecord {
    let turn = Turn {
        conversation: "c".into(),
        model: Some(model.into()),
        effort: None,
        prompt: None,
        started_ms: 0,
        ended_ms: 1,
        wall_ms: 1,
        turns: 1,
        tokens: Tokens { input, output, cache_read: 0, cache_write: 0 },
        exit: TurnExit::Completed,
    };
    RunRecord { session: session.into(), task_ref: None, provider: "claude".into(), account: None, system_prompt_sha: None, deepen: false, turn }
}

#[test]
fn a_gate_closes_once_with_who_answered_and_when() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    let early = ledger.open_gate(&request("a", 60_000), 1_000).unwrap();
    let late = ledger.open_gate(&request("b", 5_000), 2_000).unwrap();
    let answered = ledger.open_gate(&request("c", 1_000), 0).unwrap();
    let closure = Closure { answer: "no".into(), by: Answerer::Default, closed_ms: 9_000 };
    assert!(ledger.close_gate(answered, &closure).unwrap());
    assert!(!ledger.close_gate(answered, &Closure { by: Answerer::Tom, ..closure.clone() }).unwrap());
    let stored = ledger.gate(answered).unwrap().unwrap();
    assert_eq!((stored.request, stored.closure), (request("c", 1_000), Some(closure)));
    let listed: Vec<i64> = ledger.gates(0).unwrap().iter().map(|gate| gate.id).collect();
    assert_eq!(listed, [late, early, answered], "open gates come first, soonest default first");
    let recent: Vec<i64> = ledger.gates(9_001).unwrap().iter().map(|gate| gate.id).collect();
    assert_eq!(recent, [late, early]);
}

#[test]
fn spend_sums_each_session_and_prices_only_known_models() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    for record in [run("a", "claude-haiku-5-5", 1_000_000, 0), run("a", "claude-haiku-5-5", 0, 1_000_000), run("b", "mystery", 10, 5)] {
        ledger.record_run(&record).unwrap();
    }
    let spend = ledger.spend().unwrap();
    assert_eq!(spend, [SessionSpend { session: "a".into(), tokens: 2_000_000, api_usd: Some(0.6) }, SessionSpend { session: "b".into(), tokens: 15, api_usd: None }]);
}

#[test]
fn a_landing_keeps_its_time_and_an_older_ledger_gains_the_column() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE tasks(task_ref TEXT PRIMARY KEY, category TEXT, sessions TEXT NOT NULL DEFAULT '[]', wall_ms INTEGER, outcome TEXT, landed_sha TEXT, review TEXT, corrections INTEGER NOT NULL DEFAULT 0) STRICT;")
        .unwrap();
    let ledger = Ledger::open(&path).unwrap();
    ledger.record_landed("T1", "abc", 50_000).unwrap();
    assert_eq!(ledger.landed_since(50_000).unwrap(), [Landing { task_ref: "T1".into(), sha: "abc".into(), wall_ms: None, landed_ms: 50_000 }]);
    assert!(ledger.landed_since(50_001).unwrap().is_empty());
}
