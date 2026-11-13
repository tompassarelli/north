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
        ask_once: None,
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
        tool_calls: None,
        compactions: None,
        exit: TurnExit::Completed,
    };
    RunRecord { session: session.into(), task_ref: None, provider: "claude".into(), overlay: None, account: None, system_prompt_sha: None, deepen: false, turn }
}

#[test]
fn a_parent_answer_survives_reopen_delivers_once_and_blocks_defaults_strangers_and_a_gate_put_to_tom() {
    use crate::fleet::gates::{Authority, Relay, TomTurn, TypedTurn, answer_gate, default_overdue, open, orphan, relay_gate};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let legacy = super::SCHEMA.replace("answered_by TEXT,", "answered_by TEXT CHECK(answered_by IN ('tom','default')),").replace("verification INTEGER NOT NULL DEFAULT 0,\n  issue TEXT,\n  box_text TEXT,\n  ask_once TEXT\n", "verification INTEGER NOT NULL DEFAULT 0\n");
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
    let ask_once = book.open_gate(&GateRequest { default: 0, ..request("child", 0) }, 100).unwrap();
    let refused = answer_gate(&book, ask_once, 1, None, Authority { caller: Some("parent"), parent: Some("parent"), typed: None }, 200, |_, _| panic!("a parent answered a gate put to Tom")).unwrap_err();
    assert!(refused.to_string().contains("put to Tom"), "{refused}");
    assert!(book.gate(ask_once).unwrap().unwrap().is_open());
    let relayed = TypedTurn { conversation: "c-0".into(), at_ms: 150 };
    let closure = answer_gate(&book, ask_once, 1, None, Authority { caller: Some("parent"), parent: Some("parent"), typed: Some(&relayed) }, 200, |_, _| Ok(())).unwrap();
    assert_eq!(closure.by, Answerer::Tom, "a relay of Tom's typed turn closes a gate put to Tom");
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
        let refused = relay_gate(&book, relayed, number, Relay { turn: &evidence, relayer: "proxy", parent: None }, 3_000, |_, _| panic!("a refused relay delivered")).unwrap_err().user_message();
        assert!(refused.contains(why), "{refused}");
        assert!(book.gate(relayed).unwrap().unwrap().is_open());
    }
    let closure = relay_gate(&book, relayed, 2, Relay { turn: &turn(10, 2_000, "2. keep it local, thanks"), relayer: "proxy", parent: None }, 3_000, |_, text| {
        assert!(text.contains("Tom answered: keep it local\nrelayed by proxy from convo turn 10"));
        Ok(())
    }).unwrap();
    assert_eq!(closure.by, Answerer::Tom);
    let defaulted = book.open_gate(&request("proxy", 30_000), 1_000).unwrap();
    assert!(relay_gate(&book, defaulted, 1, Relay { turn: &turn(11, 2_000, "1"), relayer: "proxy", parent: Some("parent") }, 3_000, |_, _| panic!("a gate its asker's parent answers takes no relay")).is_err());
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


#[test]
fn a_closed_cell_costs_the_sum_of_the_runs_it_counts_and_names_the_earliest_of_them() {
    use super::outcome::replay_tests;
    let db = replay_tests::ledger();
    let runs = replay_tests::spend_runs(&db, &replay_tests::judged(&db));
    let prices = crate::router::pricing::PriceTable::load().unwrap();
    let closed = ["north#9", "north#12", "north#13"].map(str::to_owned).into_iter().collect();
    let report = super::summarize(&runs, &closed, &Default::default(), "2026-11-01T00:00:00Z", &prices);
    let cell = report.cells.iter().find(|cell| cell.tier == "opus-medium" && cell.category == "feature").unwrap();
    let counted: Vec<String> = cell.boxes_dearest_first().into_iter().flat_map(|closed| closed.runs.into_iter().map(|run| run.agent)).collect();
    let priced: f64 = runs.iter().filter(|run| counted.contains(&run.agent)).map(|run| prices.run_usd(run.model.as_deref(), &run.tier, run.input, run.cache_read, run.output)).sum();
    assert_eq!((cell.closed, counted.len()), (3, 3));
    assert!((cell.usd - priced).abs() < 1e-9, "cell {} against its runs {priced}", cell.usd);
    assert_eq!(cell.runs_since(), Some("2026-10-25T13:00:00Z"));
    assert!(report.dashboard("7d").starts_with("cost/closed, boxes closed in 7d, each priced at API-equivalent rates over all its runs, the earliest started 2026-10-25:"), "{}", report.dashboard("7d"));
}

fn umbrella() -> String {
    let stamped = "- [x] A run's kept box is judged by its own stamp. Done when: `cargo test` passes. Evidence: `cargo test` passed on main 1a2b3c4d5e6f on 2026-11-08, exit 0.";
    let others: String = (1..=14).map(|n| format!("- [x] Piece {n} of the umbrella, self-ticked by its worker (landed 1a2b3c4).\n")).collect();
    format!("{others}{stamped}\n- [ ] A box nobody has staffed yet.\n")
}

#[test]
fn checker_verified_reads_the_stamp_on_the_box_a_runs_brief_named_and_the_whole_issue_only_without_one() {
    use super::closed::{box_verified, checker_verified};
    let body = umbrella();
    assert!(!checker_verified(&body), "the issue-level read fails an umbrella with 14 self-ticked boxes");
    assert!(box_verified(&body, "A run's kept box is judged by its own stamp"), "the one staffed box is stamped");
    assert!(!box_verified(&body, "Piece 3 of the umbrella"), "a self-ticked box is not verified by its sibling's stamp");
    assert!(!box_verified(&body, "A box nobody has staffed yet."), "an unticked box is not kept");
    assert!(!box_verified(&body, "A box the issue does not hold"));
    let foreign = format!("{body}- [x] Another kept box. Done when: `cargo test -p north-base`. Evidence: `cargo test` passed on main 1a2b3c4d5e6f on 2026-11-08, exit 0.\n");
    assert!(!box_verified(&foreign, "Another kept box"), "a stamp for another command is not this box's own Done-when passing");
    let failed = format!("{body}- [x] A third kept box. Done when: `cargo test`. Evidence: `cargo test` passed on main 1a2b3c4d5e6f on 2026-11-08, exit 1.\n");
    assert!(!box_verified(&failed, "A third kept box"), "a stamp that records a nonzero exit is not a pass");
    let own = "- [x] Only box. Done when: `cargo test`. Evidence: `cargo test` passed on main 1a2b3c4d5e6f on 2026-11-08, exit 0.\n";
    assert!(checker_verified(own), "a box whose own Done-when command passed on main is verified");
}

#[test]
fn a_box_ticked_on_an_issue_that_stays_open_counts_as_closed() {
    use super::closed::SpendRun;
    let run = |agent: &str, item: &str, box_text: Option<&str>| SpendRun {
        agent: agent.into(),
        item: Some(item.into()),
        role: "worker".into(),
        tier: "sonnet-high".into(),
        category: "tooling".into(),
        arm: "context-400k".into(),
        box_text: box_text.map(str::to_owned),
        model: Some("claude-sonnet-5-5".into()),
        effort: String::new(),
        started: "2026-11-08T10:00:00Z".into(),
        ended: "2026-11-08T11:00:00Z".into(),
        landed: true,
        reworked: false,
        input: 1_000,
        cache_read: 0,
        output: 100,
        skills: Vec::new(),
    };
    let stamped = "A run's kept box is judged by its own stamp";
    let runs = [run("a", "north#1", Some(stamped)), run("b", "north#1", None), run("c", "north#1", Some("Piece 3 of the umbrella")), run("d", "north#1", Some("A box nobody has staffed yet."))];
    let bodies: std::collections::HashMap<String, String> = [("north#1".to_owned(), umbrella())].into();
    let report = super::summarize(&runs, &Default::default(), &bodies, "2026-11-01T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap()).verified(&bodies, None);
    assert_eq!(report.closed, 2, "the two ticked boxes count; the run naming no box and the unticked box do not, though the issue stays open");
    let read = |text: &str| report.arms.iter().find(|arm| arm.box_text.as_deref() == Some(text) && arm.closed).map(|arm| (arm.verified, arm.issue_verified));
    assert_eq!(read(stamped), Some((true, false)), "the staffed, stamped box of an umbrella reads verified");
    assert_eq!(read("Piece 3 of the umbrella"), Some((false, false)), "a self-ticked box is not verified by its sibling's stamp");
    assert_eq!(read("A box nobody has staffed yet."), None);
}

#[test]
fn corroborated_reads_a_self_ticked_box_unverified_until_the_checker_passes_its_done_when_command() {
    use north_graph::{Graph, Term};
    const SHA: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
    const COMMAND: &str = "cargo test -p north-providers corroborated";
    let box_text = "Corroborated rewards: the staffed box of an umbrella";
    let node = crate::threads::claims::box_node("north#882", box_text).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let seeded = |name: &str| {
        let path = directory.path().join(name);
        let mut graph = Graph::open(&path).unwrap();
        let mut tell = |writer: &str, t1: Term, t2: &str, t3: Term| graph.tell(writer, north_graph::Triple::new(t1, Term::keyword(t2), t3)).unwrap();
        tell("n882-worker", Term::string("n882-worker"), "claims", Term::triple(Term::string(&node), Term::keyword("done-at"), Term::string(SHA)));
        tell("mirror", Term::string("north#882"), "has-box", Term::string(&node));
        tell("mirror", Term::string(&node), "done-when", Term::string(COMMAND));
        tell("push-log", Term::string(SHA), "landed-by", Term::string("n882-worker"));
        tell("git", Term::string(SHA), "ancestor-of", Term::string("origin/main"));
        path
    };
    let path = seeded("graph.db");
    let forged = format!("- [x] {box_text}. Done when: `{COMMAND}` exits 0. Evidence: `{COMMAND}` passed on main 59ff4fb9c2d2 on 2026-11-08, exit 0.\n");
    let closed: std::collections::HashMap<String, String> = [("north#882".to_owned(), forged)].into();
    let run = super::closed::SpendRun {
        agent: "n882-worker".into(),
        item: Some("north#882".into()),
        role: "worker".into(),
        tier: "high".into(),
        category: "feature".into(),
        arm: "context-400k".into(),
        box_text: Some(box_text.into()),
        model: Some("claude-opus-5-5".into()),
        effort: String::new(),
        started: "2026-11-08T10:00:00Z".into(),
        ended: "2026-11-08T11:00:00Z".into(),
        landed: true,
        reworked: false,
        input: 1_000,
        cache_read: 0,
        output: 100,
        skills: Vec::new(),
    };
    let verified = |path: &std::path::Path| {
        let corroborated = super::corroborated::checked_boxes(path).unwrap();
        let items = closed.keys().cloned().collect();
        let report = super::summarize(std::slice::from_ref(&run), &items, &Default::default(), "2026-11-01T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap()).verified(&closed, corroborated.as_ref());
        report.arms[0].verified
    };
    assert!(!verified(&path), "a self-ticked box whose own stamp claims a pass reads unverified while no checker assertion corroborates it");
    let checked = |path: &std::path::Path, sha: &str, issue: &str, passed: bool| {
        let mut graph = Graph::open(path).unwrap();
        let mut tell = |subject: &str, relation: &str, object: Term| graph.tell("checker", north_graph::Triple::new(Term::string(subject), Term::keyword(relation), object)).unwrap();
        tell(sha, if passed { "check-passed" } else { "check-failed" }, Term::string(COMMAND));
        tell(sha, "checked-for", Term::string(issue));
        if passed {
            tell(issue, "passed-check", Term::triple(Term::string(COMMAND), Term::keyword("on"), Term::string(sha)));
        }
    };
    let contains = |path: &std::path::Path, claimed: &str, head: &str| Graph::open(path).unwrap().tell("git", north_graph::Triple::new(Term::string(claimed), Term::keyword("in-tree-of"), Term::string(head))).unwrap();
    let main = "211159d7ca5b0e8f0c4d6a1e2b3c4d5e6f708192";
    let lane = "7a7a1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7";
    let on_main = |path: &std::path::Path, sha: &str| Graph::open(path).unwrap().tell("git", north_graph::Triple::new(Term::string(sha), Term::keyword("ancestor-of"), Term::string("origin/main"))).unwrap();
    let before = "3c1d9e0a47b5f2861d0c9e8b7a6f5e4d3c2b1a09";
    checked(&path, "0840a94290aa1b2c3d4e5f60718293a4b5c6d7e8", "north#9", true);
    assert!(!verified(&path), "a pass of the box's command checked for another issue does not corroborate it");
    Graph::open(&path).unwrap().tell("n882-worker", north_graph::Triple::new(Term::string("north#9"), Term::keyword("has-box"), Term::string(&node))).unwrap();
    assert!(!verified(&path), "a has-box the worker tells to join its box to another issue's pass does not corroborate it");
    checked(&path, main, "north#9", true);
    checked(&path, main, "north#882", false);
    contains(&path, SHA, main);
    assert!(!verified(&path), "another issue's pass on the main head the box's own check failed on does not corroborate it");
    checked(&path, before, "north#882", true);
    assert!(!verified(&path), "the box's own pass on a commit before the claimed one, which does not hold the claimed change, does not corroborate it");
    checked(&path, lane, "north#882", true);
    contains(&path, SHA, lane);
    assert!(!verified(&path), "the box's own pass on a checkout head that is not on origin/main, though it holds the claimed commit, does not corroborate it");
    checked(&path, main, "north#882", true);
    assert!(!verified(&path), "a later head git has not told to be on origin/main does not corroborate it");
    on_main(&path, main);
    assert!(verified(&path), "the checker passing the box's Done-when command on a later main head than the claimed commit corroborates the claim");
    let exact = seeded("exact.db");
    checked(&exact, SHA, "north#882", true);
    assert!(!verified(&exact), "a pass on the claimed commit corroborates only once git tells the commit is in that tree");
    contains(&exact, SHA, SHA);
    assert!(verified(&exact), "the checker passing the box's Done-when command on the claimed commit itself corroborates the claim");
    assert!(verified(&directory.path().join("absent.db")), "where no graph exists the box's stamp is read");
}

#[test]
fn a_dash_window_counts_only_runs_that_started_inside_it_and_never_a_run_stamped_after_its_end() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    let insert = |agent: &str, started: &str, ended: &str| {
        db.execute(
            "INSERT INTO runs(agent, item, tier, category, started, ended, outcome, minutes, model, input_tokens, tokens, brief_text) VALUES (?1, 'life#1', 'sonnet', 'feature', ?2, ?3, 'done', 60, 'claude-sonnet-5-5', 1000, 100, '')",
            rusqlite::params![agent, started, ended],
        )
        .unwrap();
    };
    insert("inside", "2026-11-09T05:00:00Z", "2026-11-09T06:00:00Z");
    insert("before", "2026-11-08T05:00:00Z", "2026-11-08T06:00:00Z");
    insert("after", "2026-11-09T13:00:00Z", "2026-11-09T14:00:00Z");
    let now = crate::foundation::clock::Timestamp::parse("2026-11-09T12:00:00Z").unwrap();
    let runs = super::closed_source::runs_inside_in(&path, "24h", now).unwrap();
    assert_eq!(runs.iter().map(|run| run.agent.as_str()).collect::<Vec<_>>(), ["inside"], "a run started before the window or stamped after its end is not in it");
}

#[test]
fn handovers_count_the_runs_that_follow_another_and_price_their_first_turn_cache_write_per_week() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    let insert = |agent: &str, follows: Option<&str>, started: &str| {
        db.execute(
            "INSERT INTO runs(agent, item, follows, tier, category, started, ended, outcome, minutes, model, input_tokens, cache_read_tokens, peak_ctx, tokens, brief_text) VALUES (?1, 'life#1', ?2, 'sonnet', 'feature', ?3, ?3, 'done', 60, 'claude-sonnet-5-5', 1000000, 600000, 200000, 100, '')",
            rusqlite::params![agent, follows, started],
        )
        .unwrap();
    };
    insert("first", None, "2026-11-09T05:00:00Z");
    insert("handed", Some("first"), "2026-11-09T06:00:00Z");
    let now = crate::foundation::clock::Timestamp::parse("2026-11-09T12:00:00Z").unwrap();
    let day = super::closed_source::handovers_in(&path, "24h", now).unwrap();
    let week = super::closed_source::handovers_in(&path, "7d", now).unwrap();
    assert_eq!(week.per_week, 1.0);
    assert_eq!(day.per_week, 7.0, "one handover in a day is seven in a week");
    let price = crate::router::pricing::PriceTable::load().unwrap().cache_write_usd(Some("claude-sonnet-5-5"), "sonnet", 200_000);
    assert!(price > 0.0 && (week.first_turn_cache_write_usd - price).abs() < 1e-9, "the first turn writes no more than the run's 200k peak context: {week:?}");
}

#[test]
fn run_times_names_each_run_its_wall_minutes_and_the_eta_its_brief_stated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    let insert = |agent: &str, minutes: u32, eta: Option<u32>| {
        db.execute(
            "INSERT INTO runs(agent, tier, category, ended, minutes, eta_min, outcome) VALUES (?1, 'claude:sonnet-high', 'feature', '2026-11-10T00:00:00Z', ?2, ?3, 'not done')",
            rusqlite::params![agent, minutes, eta],
        )
        .unwrap();
    };
    insert("n940-d0900", 36, Some(18));
    insert("n940-d0901", 5, None);
    drop(db);
    let times = super::run_times_in(&path).unwrap();
    assert_eq!(times.get("n940-d0900"), Some(&super::RunTime { minutes: 36, eta_min: 18 }));
    assert_eq!(times.get("n940-d0901"), None, "a run whose brief stated no ETA climbs on no ETA signal");
}

#[test]
fn a_fable_5_2_run_is_priced_so_fleet_spend_prints_dollars_instead_of_unmeasured() {
    use crate::fleet::books::Books;
    let directory = tempfile::tempdir().unwrap();
    let ledger = ledger(&directory);
    ledger.record_run(&run("fable", "claude-fable-5-2", 1_000_000, 0)).unwrap();
    let spend = ledger.spend().unwrap();
    let fable = spend.iter().find(|spend| spend.session == "fable").unwrap();
    assert_eq!(fable.tokens, 1_000_000);
    assert!(fable.api_usd.is_some_and(|usd| usd > 0.0), "a Fable session reads {:?}", fable.api_usd);
}

fn worker_run(agent: &str, item: &str, started: &str, ended: &str, landed: bool) -> super::closed::SpendRun {
    super::closed::SpendRun {
        agent: agent.into(),
        item: Some(item.into()),
        role: "worker".into(),
        tier: "sonnet-high".into(),
        category: "feature".into(),
        arm: "context-400k".into(),
        box_text: None,
        model: Some("claude-sonnet-5-5".into()),
        effort: "high".into(),
        started: started.into(),
        ended: ended.into(),
        landed,
        reworked: false,
        input: 1_000,
        cache_read: 0,
        output: 100,
        skills: Vec::new(),
    }
}

const LEAF: &str = "Count a leaf closed by its check.\n\nDone when: `cargo nextest run -p north-providers -E 'test(/leaf/)'` exits 0.";
const LEAF_COMMAND: &str = "cargo nextest run -p north-providers -E 'test(/leaf/)'";

fn report_of(runs: Vec<super::closed::SpendRun>, closed: &[(&str, super::Closed)]) -> super::ClosedReport {
    let closed: std::collections::HashMap<String, super::Closed> = closed.iter().map(|(item, issue)| ((*item).to_owned(), issue.clone())).collect();
    let sources = super::closed_source::Inputs { runs, native: Default::default(), closed: Default::default(), corroborated: None, catalog: &crate::threads::catalog::CATALOG };
    super::closed_source::report_between(&sources, "2026-11-01T00:00:00Z", "2026-11-09T00:00:00Z", move |_, _, _| Ok(super::closed_source::Listing { closed: closed.clone(), open: Default::default() }), &crate::router::pricing::PriceTable::load().unwrap()).unwrap()
}

#[test]
fn a_closed_leaf_counts_as_one_closed_box() {
    let runs = vec![worker_run("leaf-worker", "north#5", "2026-11-08T10:00:00Z", "2026-11-08T11:00:00Z", true), worker_run("hand-closed-worker", "north#6", "2026-11-08T10:00:00Z", "2026-11-08T11:00:00Z", true)];
    let by_check = super::Closed { by_check: Some(LEAF_COMMAND.to_owned()), ..super::Closed::of_body(LEAF) };
    let report = report_of(runs, &[("north#5", by_check), ("north#6", super::Closed::of_body(LEAF))]);
    let verified = |item: &str| report.arms.iter().find(|arm| arm.item == item).map(|arm| (arm.closed, arm.verified)).unwrap();
    assert_eq!(report.closed, 2);
    assert_eq!(verified("north#5"), (true, true), "a leaf the lander closed by its Done-when command is a box graded by its own check");
    assert_eq!(verified("north#6"), (true, false), "a leaf closed by hand counts as closed and is not graded");
    assert_eq!(super::Closed::check_named_in(["Main is green", &format!("Closed by the landing of 4cd01a47cd12: its Done-when command `{LEAF_COMMAND}` passed on that commit.")]), Some(LEAF_COMMAND.to_owned()));
}

#[test]
fn a_run_that_created_children_counts_as_a_split() {
    let runs = vec![
        worker_run("parent-worker", "north#9", "2026-11-08T10:00:00Z", "2026-11-08T11:00:00Z", false),
        worker_run("later-worker", "north#9", "2026-11-08T13:00:00Z", "2026-11-08T14:00:00Z", false),
        worker_run("child-worker", "north#10", "2026-11-08T11:30:00Z", "2026-11-08T12:00:00Z", true),
    ];
    let child = super::Closed { parent: Some("north#9".to_owned()), created: "2026-11-08T10:45:00Z".to_owned(), by_check: Some(LEAF_COMMAND.to_owned()), ..super::Closed::of_body(LEAF) };
    let report = report_of(runs, &[("north#10", child)]);
    assert_eq!(report.closed, 1);
    assert_eq!(report.splits.iter().map(|split| split.item.as_str()).collect::<Vec<_>>(), ["north#9"]);
    let unclosed: f64 = report.cells.iter().map(|cell| cell.unclosed_usd).sum();
    let later = report.boxes.iter().find(|spent| spent.item == "north#9" && !spent.closed).map(|spent| spent.usd).unwrap();
    assert!((unclosed - later).abs() < 1e-9, "only the run that made no child is spend that closed nothing: {unclosed} against {later}");
    assert!(report.dashboard("7d").contains("split 1 ("), "{}", report.dashboard("7d"));
}

#[test]
fn a_gate_closed_by_two_sweeps_at_once_is_a_lost_race_and_not_an_illegal_move() {
    use crate::fleet::gate_lifecycle::GatePhase;
    use crate::fleet::gates::Closure;

    let directory = tempfile::tempdir().unwrap();
    let book = ledger(&directory);
    let id = book.open_gate(&GateRequest { default: 0, default_after_ms: 0, ..request("proxy", 0) }, 1_000).unwrap();
    let seen = book.gate(id).unwrap().unwrap().phase;
    let closure = Closure { answer: "orphaned".into(), by: Answerer::Orphaned, closed_ms: 2_000 };

    assert!(book.close_from(id, Some(seen), &closure).unwrap());
    book.move_gate(id, GatePhase::Orphaned, "answer delivered", 2_001).unwrap();

    assert!(!book.close_from(id, Some(seen), &closure).unwrap());
    assert!(!book.close_gate(id, &closure).unwrap());
    let moves = north_base::lifecycle::Store::open(&directory.path().join("ledger.db")).unwrap().history("gate", &id.to_string()).unwrap();
    assert!(moves.iter().all(|logged| !logged.refused), "{moves:?}");
    assert_eq!(book.gate(id).unwrap().unwrap().phase, GatePhase::Orphaned);
}
