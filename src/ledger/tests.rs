use rusqlite::Connection;

use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::gates::{Answerer, Closure, GateBook, GateRequest};
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::turns::{RunRecord, RunSink, Tokens, Turn, TurnExit};

use super::Ledger;

fn ledger(directory: &tempfile::TempDir) -> Ledger {
    Ledger::open(&directory.path().join("ledger.db")).unwrap()
}

fn request(session: &str, default_after_ms: u64) -> GateRequest {
    GateRequest {
        verification: false,
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
    assert!(ledger.record_landing("T1", "abc", 50_000, None).unwrap());
    assert!(!ledger.record_landing("T1", "abc", 60_000, Some(1)).unwrap(), "observing the same landing again keeps its first time");
    assert_eq!(ledger.landed_since(50_000).unwrap(), [Landing { task_ref: "T1".into(), sha: "abc".into(), wall_ms: None, landed_ms: 50_000 }]);
    assert!(ledger.landed_since(50_001).unwrap().is_empty());
}

#[test]
fn runs_are_numbered_per_session_and_a_claimed_receipt_stays_claimed_across_a_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    let numbers: Vec<u64> = [run("a", "m", 1, 1), run("b", "m", 1, 1), run("a", "m", 1, 1)].iter().map(|record| ledger.record_run(record).unwrap()).collect();
    assert_eq!(numbers, [1, 1, 2]);
    assert!(ledger.claim_receipt("a", "turn:2", 5).unwrap());
    assert!(!ledger.claim_receipt("a", "turn:2", 6).unwrap());
    assert!(ledger.claim_receipt("b", "turn:2", 6).unwrap(), "claims are per session");
    drop(ledger);
    assert!(!self::ledger(&directory).claim_receipt("a", "turn:2", 7).unwrap(), "a restarted host must not claim a sent receipt again");
}

#[test]
fn a_parent_answer_survives_reopen_delivers_once_and_blocks_defaults_and_strangers() {
    use crate::fleet::gates::{Authority, answer_gate, default_overdue, open};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let legacy = super::SCHEMA.replace("answered_by TEXT,", "answered_by TEXT CHECK(answered_by IN ('tom','default')),");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(&legacy).unwrap();
    conn.execute(r#"INSERT INTO gates VALUES(1, 'old', 'Old?', '["yes","no"]', 1, 2, 10, 0, 'yes', 'tom', 1, 0)"#, []).unwrap();
    drop(conn);
    let book = Ledger::open(&path).unwrap();
    assert_eq!(book.gate(1).unwrap().unwrap().closure.unwrap().by, Answerer::Tom);
    let id = open(&book, &request("child", 30_000), Some("parent"), None, 100, |parent, notice| {
        assert_eq!(parent, "parent");
        assert!(notice.starts_with("[gate "));
    }).unwrap();
    for caller in ["stranger", "child"] {
        assert!(answer_gate(&book, id, 1, None, Authority { caller: Some(caller), parent: Some("parent") }, 200, |_, _| panic!("unauthorized delivery")).is_err());
        assert!(book.gate(id).unwrap().unwrap().is_open());
    }
    for number in [0, 3] {
        assert!(answer_gate(&book, id, number, None, Authority { caller: Some("parent"), parent: Some("parent") }, 200, |_, _| panic!("invalid choice delivery")).is_err());
    }
    let mut delivered = Vec::new();
    let closure = answer_gate(&book, id, 1, Some("proceed now"), Authority { caller: Some("parent"), parent: Some("parent") }, 200, |session, text| {
        delivered.push((session.to_owned(), text.to_owned()));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by.label(), "parent");
    assert_eq!(delivered, [("child".to_owned(), format!("[gate {id}] parent answered: yes\nproceed now\nYour question: Rent?"))]);
    drop(book);
    let book = Ledger::open(&path).unwrap();
    assert_eq!(book.gate(id).unwrap().unwrap().closure, Some(closure));
    assert!(answer_gate(&book, id, 2, None, Authority { caller: Some("parent"), parent: Some("parent") }, 300, |_, _| panic!("duplicate delivery")).is_err());
    assert!(default_overdue(&book, 40_000, |_, _| panic!("late default delivery")).unwrap().is_empty());
    let tom = book.open_gate(&request("root", 30_000), 100).unwrap();
    let closure = answer_gate(&book, tom, 2, None, Authority { caller: None, parent: None }, 200, |session, text| {
        assert_eq!(session, "root");
        assert!(text.contains("Tom answered: no"));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by, Answerer::Tom);
}

#[test]
fn verification_gates_stay_out_of_toms_decisions_after_the_ledger_reopens() {
    use crate::fleet::gates::decisions;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let book = Ledger::open(&path).unwrap();
    let work = book.open_gate(&request("work", 30_000), 1).unwrap();
    let test = book.open_gate(&GateRequest { verification: true, ..request("test", 30_000) }, 2).unwrap();
    drop(book);
    let book = Ledger::open(&path).unwrap();
    let visible = decisions(book.gates(0).unwrap());
    assert_eq!(visible.iter().map(|gate| gate.id).collect::<Vec<_>>(), [work]);
    assert!(book.gate(test).unwrap().unwrap().is_open(), "the test parent can still answer its own gate");
    assert!(book.gate(test).unwrap().unwrap().request.verification);
}

#[test]
fn a_landed_task_links_the_runs_of_the_session_it_is_named_for_or_lists() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    for session in ["lane-a", "lane-b", "lane-c"] {
        ledger.record_run(&run(session, "claude-opus-5-5", 1, 1)).unwrap();
    }
    ledger.join_task("north#9", "lane-b").unwrap();
    for (task, sha) in [("lane-a", "sha-a"), ("north#9", "sha-b"), ("ghost", "sha-c")] {
        ledger.record_landing(task, sha, 5_000, None).unwrap();
    }
    assert_eq!(ledger.landing_links(0).unwrap(), (2, 3), "the ghost landing has no run; the session-named and session-listed ones do");
    assert_eq!(ledger.landing_links(6_000).unwrap(), (0, 0));
    let linked: Vec<Option<String>> = ledger.runs().unwrap().into_iter().map(|run| run.task_ref).collect();
    assert_eq!(linked, [Some("lane-a".to_owned()), Some("north#9".to_owned()), None]);
}
