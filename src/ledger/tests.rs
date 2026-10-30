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
        issue: None,
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
    assert!(ledger.record_landing("T1", "abc", 50_000, None, None).unwrap());
    assert!(!ledger.record_landing("T1", "abc", 60_000, Some(1), None).unwrap(), "observing the same landing again keeps its first time");
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
    let legacy = super::SCHEMA.replace("answered_by TEXT,", "answered_by TEXT CHECK(answered_by IN ('tom','default')),").replace("verification INTEGER NOT NULL DEFAULT 0,\n  issue TEXT\n", "verification INTEGER NOT NULL DEFAULT 0\n");
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
fn verification_gates_stay_out_of_toms_decisions_and_a_gate_keeps_its_issue_after_the_ledger_reopens() {
    use crate::fleet::gates::decisions;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let book = Ledger::open(&path).unwrap();
    let work = book.open_gate(&GateRequest { issue: Some("north#370".into()), ..request("work", 30_000) }, 1).unwrap();
    let test = book.open_gate(&GateRequest { verification: true, ..request("test", 30_000) }, 2).unwrap();
    drop(book);
    let book = Ledger::open(&path).unwrap();
    let visible = decisions(book.gates(0).unwrap());
    assert_eq!(visible.iter().map(|gate| gate.id).collect::<Vec<_>>(), [work]);
    assert!(book.gate(test).unwrap().unwrap().is_open(), "the test parent can still answer its own gate");
    assert!(book.gate(test).unwrap().unwrap().request.verification);
    assert_eq!(visible[0].request.issue.as_deref(), Some("north#370"));
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
        ledger.record_landing(task, sha, 5_000, None, None).unwrap();
    }
    assert_eq!(ledger.landing_links(0).unwrap(), (2, 3), "the ghost landing has no run; the session-named and session-listed ones do");
    assert_eq!(ledger.landing_links(6_000).unwrap(), (0, 0));
    let linked: Vec<Option<String>> = ledger.runs().unwrap().into_iter().map(|run| run.task_ref).collect();
    assert_eq!(linked, [Some("lane-a".to_owned()), Some("north#9".to_owned()), None]);
}

#[test]
fn a_landing_records_its_reviewer_and_one_check_row_and_a_replay_adds_none() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    assert!(ledger.record_landing("north#1", "sha-1", 5_000, None, Some("reviewer-one")).unwrap());
    assert!(!ledger.record_landing("north#1", "sha-1", 6_000, None, Some("reviewer-one")).unwrap());
    assert!(ledger.record_landing("north#2", "sha-2", 7_000, None, None).unwrap());
    let tasks = ledger.tasks().unwrap();
    let figures: Vec<_> = tasks.iter().map(|task| (task.task_ref.as_str(), task.review.as_deref(), task.checks, task.last_check_exit, task.last_check_sha.as_deref())).collect();
    assert_eq!(figures, [("north#1", Some("reviewer-one"), 1, Some(0), Some("sha-1")), ("north#2", None, 1, Some(0), Some("sha-2"))]);
}

#[test]
fn a_later_landing_without_a_reviewer_keeps_the_recorded_one() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    assert!(ledger.record_landing("north#3", "sha-1", 5_000, None, Some("reviewer-one")).unwrap());
    assert!(ledger.record_landing("north#3", "sha-2", 6_000, None, None).unwrap());
    let task = &ledger.tasks().unwrap()[0];
    assert_eq!((task.review.as_deref(), task.checks, task.last_check_sha.as_deref()), (Some("reviewer-one"), 2, Some("sha-2")));
}

fn briefed(session: &str, prompt: &str, task_ref: Option<&str>) -> RunRecord {
    let mut record = run(session, "claude-opus-5-5", 1, 1);
    record.turn.prompt = Some(prompt.into());
    record.task_ref = task_ref.map(str::to_owned);
    record
}

#[test]
fn a_landed_task_links_the_runs_of_a_session_whose_brief_or_task_flag_names_it() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    ledger.record_run(&briefed("by-flag", "go", Some("north#1"))).unwrap();
    ledger.record_run(&briefed("by-flag", "more", None)).unwrap();
    ledger.record_run(&briefed("late-brief", "HOLD-SPAWNS none", None)).unwrap();
    ledger.record_run(&briefed("late-brief", "Item: north#2 Category: tooling\nDone when: x", None)).unwrap();
    ledger.record_run(&briefed("late-brief", "ok", None)).unwrap();
    ledger.record_run(&briefed("no-brief", "Item mentioned: nothing", None)).unwrap();
    for (task, sha) in [("north#1", "sha-1"), ("north#2", "sha-2"), ("north#3", "sha-3")] {
        ledger.record_landing(task, sha, 5_000, None, None).unwrap();
    }
    assert_eq!(ledger.landing_links(0).unwrap(), (2, 3), "north#3 has no run; the flag and the late brief link theirs");
    let linked: Vec<Option<String>> = ledger.runs().unwrap().into_iter().map(|run| run.task_ref).collect();
    let task = |name: &str| Some(name.to_owned());
    assert_eq!(linked, [task("north#1"), task("north#1"), task("north#2"), task("north#2"), task("north#2"), None]);
}

#[test]
fn migrating_the_ledger_links_runs_recorded_before_their_brief_was_read() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let first = Ledger::open(&path).unwrap();
    first.record_run(&briefed("old", "Item: north#4", None)).unwrap();
    first.with(|conn| conn.execute("UPDATE runs SET task_ref = NULL", [])).unwrap();
    first.with(|conn| conn.pragma_update(None, "user_version", 0)).unwrap();
    assert_eq!(first.runs().unwrap()[0].task_ref, None);
    drop(first);
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(reopened.runs().unwrap()[0].task_ref.as_deref(), Some("north#4"));
}

#[test]
fn a_run_recorded_after_its_sessions_landing_is_linked_without_reopening_the_ledger() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    ledger.record_landing("lane-late", "sha-1", 5_000, None, None).unwrap();
    ledger.join_task("north#8", "lane-listed").unwrap();
    ledger.record_landing("north#8", "sha-2", 5_000, None, None).unwrap();
    ledger.record_run(&run("lane-late", "claude-opus-5-5", 1, 1)).unwrap();
    ledger.record_run(&run("lane-listed", "claude-opus-5-5", 1, 1)).unwrap();
    ledger.record_run(&run("lane-other", "claude-opus-5-5", 1, 1)).unwrap();
    assert_eq!(ledger.landing_links(0).unwrap(), (2, 2));
    let linked: Vec<Option<String>> = ledger.runs().unwrap().into_iter().map(|run| run.task_ref).collect();
    assert_eq!(linked, [Some("lane-late".to_owned()), Some("north#8".to_owned()), None]);
}

#[test]
fn migrating_a_locked_ledger_retries_until_the_lock_clears_and_fails_once_the_wait_runs_out() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 0).unwrap();
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let refused = Ledger::open_within(&path, std::time::Duration::from_millis(200)).err().unwrap().to_string();
    assert!(refused.contains("database is locked"), "{refused}");

    let started = std::time::Instant::now();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        holder.execute_batch("COMMIT").unwrap();
    });
    Ledger::open_within(&path, std::time::Duration::from_secs(5)).unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(400), "{:?}", started.elapsed());
    release.join().unwrap();
}

#[test]
fn readers_never_meet_a_lock_while_a_writer_holds_a_write_transaction_for_two_seconds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE; INSERT INTO events(session, kind, payload, recorded_ms) VALUES('s', 'k', '{}', 1)").unwrap();
    let started = std::time::Instant::now();
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
    std::thread::sleep(std::time::Duration::from_secs(2));
    holder.execute_batch("COMMIT").unwrap();
    for reader in readers {
        reader.join().unwrap().unwrap();
    }
    assert!(started.elapsed() >= std::time::Duration::from_secs(2));
}

#[test]
fn an_open_of_a_migrated_ledger_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    let before = std::fs::metadata(path.with_extension("db-wal")).map(|meta| meta.len()).unwrap_or(0);
    Ledger::open(&path).unwrap();
    let after = std::fs::metadata(path.with_extension("db-wal")).map(|meta| meta.len()).unwrap_or(0);
    assert_eq!(before, after);
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
