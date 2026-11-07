use rusqlite::Connection;

use crate::fleet::gates::{Answerer, GateBook, GateRequest};
use crate::fleet::turns::{RunRecord, RunSink, Tokens, Turn, TurnExit};

use super::Ledger;

fn ledger(directory: &tempfile::TempDir) -> Ledger {
    Ledger::open(&directory.path().join("ledger.db")).unwrap()
}

fn request(session: &str, default_after_ms: u64) -> GateRequest {
    GateRequest {
        verification: false,
        issue: None,
        box_text: None,
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
fn a_parent_answer_survives_reopen_delivers_once_and_blocks_defaults_and_strangers() {
    use crate::fleet::gates::{Authority, TomTurn, TypedTurn, answer_gate, default_overdue, open, orphan, relay_gate};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let legacy = super::SCHEMA.replace("answered_by TEXT,", "answered_by TEXT CHECK(answered_by IN ('tom','default')),").replace("verification INTEGER NOT NULL DEFAULT 0,\n  issue TEXT,\n  box_text TEXT\n", "verification INTEGER NOT NULL DEFAULT 0\n");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(&legacy).unwrap();
    conn.execute(r#"INSERT INTO gates VALUES(1, 'old', 'Old?', '["yes","no"]', 1, 2, 10, 0, 'yes', 'tom', 1, 0)"#, []).unwrap();
    drop(conn);
    let book = Ledger::open(&path).unwrap();
    assert_eq!(book.gate(1).unwrap().unwrap().closure.unwrap().by, Answerer::Tom);
    assert_eq!(book.gate(1).unwrap().unwrap().request.issue, None, "a gate from before --issue decides no issue");
    let id = open(&book, &request("child", 30_000), Some("parent"), None, 100, |parent, notice| {
        assert_eq!(parent, "parent");
        assert!(notice.starts_with("[gate "));
    }).unwrap();
    for caller in ["stranger", "child"] {
        assert!(answer_gate(&book, id, 1, None, Authority { caller: Some(caller), parent: Some("parent"), typed: None }, 200, |_, _| panic!("unauthorized delivery")).is_err());
        assert!(book.gate(id).unwrap().unwrap().is_open());
    }
    for number in [0, 3] {
        assert!(answer_gate(&book, id, number, None, Authority { caller: Some("parent"), parent: Some("parent"), typed: None }, 200, |_, _| panic!("invalid choice delivery")).is_err());
    }
    let mut delivered = Vec::new();
    let closure = answer_gate(&book, id, 1, Some("proceed now"), Authority { caller: Some("parent"), parent: Some("parent"), typed: None }, 200, |session, text| {
        delivered.push((session.to_owned(), text.to_owned()));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by.label(), "parent");
    assert_eq!(delivered, [("child".to_owned(), format!("[gate {id}] parent answered: yes\nproceed now\nYour question: Rent?"))]);
    drop(book);
    let book = Ledger::open(&path).unwrap();
    assert_eq!(book.gate(id).unwrap().unwrap().closure, Some(closure));
    assert!(answer_gate(&book, id, 2, None, Authority { caller: Some("parent"), parent: Some("parent"), typed: None }, 300, |_, _| panic!("duplicate delivery")).is_err());
    assert!(default_overdue(&book, 40_000, |_, _| panic!("late default delivery")).unwrap().is_empty());
    let tom = book.open_gate(&request("root", 30_000), 100).unwrap();
    let closure = answer_gate(&book, tom, 2, None, Authority { caller: None, parent: None, typed: None }, 200, |session, text| {
        assert_eq!(session, "root");
        assert!(text.contains("Tom answered: no"));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by, Answerer::Tom);
    let own = book.open_gate(&request("proxy", 30_000), 1_000).unwrap();
    for typed in [None, Some(TypedTurn { conversation: "c-1".into(), at_ms: 900 })] {
        assert!(answer_gate(&book, own, 1, None, Authority { caller: Some("proxy"), parent: None, typed: typed.as_ref() }, 3_000, |_, _| panic!("a proxy approved its own gate")).is_err());
    }
    let typed = TypedTurn { conversation: "c-1".into(), at_ms: 2_000 };
    let closure = answer_gate(&book, own, 1, None, Authority { caller: Some("proxy"), parent: None, typed: Some(&typed) }, 3_000, |_, text| {
        assert!(text.contains("Tom answered: yes\nrelayed by proxy from Tom's message in conversation c-1 at 1970-01-01T00:00:02"), "{text}");
        Ok(())
    }).unwrap();
    assert_eq!(closure.by, Answerer::Tom);
    let ask =GateRequest { default: 0, default_after_ms: 0, options: vec!["encrypt it".into(), "keep it local".into()], ..request("proxy", 0) };
    let relayed = book.open_gate(&ask, 1_000).unwrap();
    let turn = |id: i64, at_ms: u64, text: &str| TomTurn { id, at_ms, text: text.into() };
    let refusals = [(turn(7, 900, "2. keep it local"), 2, "before the gate opened"), (turn(8, 2_000, "1. encrypt it"), 2, "names option 1, not 2"), (turn(9, 2_000, "sounds fine"), 2, "names no single option")];
    for (evidence, number, why) in refusals {
        let refused = relay_gate(&book, relayed, number, &evidence, "proxy", 3_000, |_, _| panic!("a refused relay delivered")).unwrap_err().user_message();
        assert!(refused.contains(why), "{refused}");
        assert!(book.gate(relayed).unwrap().unwrap().is_open());
    }
    let closure = relay_gate(&book, relayed, 2, &turn(10, 2_000, "2. keep it local, thanks"), "proxy", 3_000, |_, text| {
        assert!(text.contains("Tom answered: keep it local\nrelayed by proxy from convo turn 10"));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by, Answerer::Tom);
    let defaulted = book.open_gate(&request("proxy", 30_000), 1_000).unwrap();
    assert!(relay_gate(&book, defaulted, 1, &turn(11, 2_000, "1"), "proxy", 3_000, |_, _| panic!("a gate with a default takes no relay")).is_err());
    let orphaned = book.open_gate(&ask, 1_000).unwrap();
    let closure = orphan(&book, orphaned, 4_000).unwrap();
    assert_eq!((closure.by.clone(), book.gate(orphaned).unwrap().unwrap().closure), (Answerer::Orphaned, Some(closure)));
}

#[test]
fn readers_never_meet_a_lock_while_a_writer_holds_a_write_transaction_for_two_seconds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE; INSERT INTO events(session, kind, payload, recorded_ms) VALUES('s', 'k', '{}', 1)").unwrap();
    let readers: Vec<_> = (0..24)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let reader = Ledger::open_within(&path, std::time::Duration::from_millis(100))?;
                reader.runs()?;
                reader.tasks().map(drop)
            })
        })
        .collect();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(60), || readers.iter().all(|reader| reader.is_finished())), "a reader waited on the writer's open transaction");
    holder.execute_batch("COMMIT").unwrap();
    for reader in readers {
        reader.join().unwrap().unwrap();
    }
}

#[test]
fn concurrent_writers_wait_their_turn_instead_of_failing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    let writers: Vec<_> = (0..8)
        .map(|index| {
            let path = path.clone();
            std::thread::spawn(move || {
                let writer = Ledger::open(&path)?;
                for turn in 0..10 {
                    writer.record_run(&run(&format!("lane-{index}"), "claude-opus-5-5", turn, 1))?;
                }
                Ok::<(), crate::error::NorthError>(())
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap().unwrap();
    }
    assert_eq!(Ledger::open(&path).unwrap().runs().unwrap().len(), 80);
}

