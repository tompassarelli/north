use rusqlite::Connection;

use crate::fleet::gates::{Answerer, GateBook, GateRequest};

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
    let refusals = [(turn(7, 900, "2. keep it local"), 2, "before the gate opened"), (turn(8, 2_000, "1. encrypt it"), 2, "names option 1, not 2"), (turn(9, 2_000, "sounds fine"), 2, "names no single option"), (turn(12, 2_000, "see gate 261 and the 290 verbs, then step 1 of 10: go with 1."), 2, "names option 1, not 2"), (turn(13, 2_000, "1. 2."), 2, "names no single option"), (turn(14, 2_000, "wait 1 more day, give it a day or 2"), 2, "names no single option"), (turn(16, 2_000, "what does option 2 do?"), 2, "names no single option"), (turn(17, 2_000, "not 2"), 2, "names no single option"), (turn(18, 2_000, "don't go with 1"), 2, "names no single option"), (turn(19, 2_000, "why would we pick 2"), 2, "names no single option"), (turn(15, 2_000, "> release condition 1 is open\nok"), 2, "names no single option")];
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
    let gap = "Fleet screen gap 1";
    let runs = [run("a", "north#1", Some(stamped)), run("b", "north#1", None), run("c", "north#1", Some("Piece 3 of the umbrella")), run("d", "north#1", Some("A box nobody has staffed yet.")), run("e", "north#1", Some(gap))];
    let body = format!("{}- [x] Fleet screen gap 13 of the umbrella, self-ticked by its worker.\n- [ ] Fleet screen gap 1 of the umbrella, not ticked yet.\n", umbrella());
    let bodies: std::collections::HashMap<String, String> = [("north#1".to_owned(), body)].into();
    let report = super::summarize(&runs, &Default::default(), &bodies, "2026-11-01T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap()).verified(&bodies, None);
    assert_eq!(report.closed, 2, "the two ticked boxes count; the run naming no box and the unticked box do not, though the issue stays open");
    assert!(!report.arms.iter().any(|arm| arm.closed && arm.box_text.as_deref() == Some(gap)), "a run naming gap 1 does not count the ticked gap 13 box as its own while its own box is unticked");
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
    let split_one = "Split short name one";
    let split_two = "Split short name two";
    let node_one = crate::threads::claims::box_node("north#882", &format!("{split_one}. Done when: `{COMMAND}`")).unwrap();
    let node_two = crate::threads::claims::box_node("north#882", &format!("{split_two}. Done when: `{COMMAND}`")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let seeded = |name: &str| {
        let path = directory.path().join(name);
        let mut graph = Graph::open(&path).unwrap();
        let mut tell = |writer: &str, t1: Term, t2: &str, t3: Term| graph.tell(writer, north_graph::Triple::new(t1, Term::keyword(t2), t3)).unwrap();
        for id in [&node, &node_one, &node_two] {
            tell("n882-worker", Term::string("n882-worker"), "claims", Term::triple(Term::string(id), Term::keyword("done-at"), Term::string(SHA)));
            tell("mirror", Term::string("north#882"), "has-box", Term::string(id));
            tell("mirror", Term::string(id), "done-when", Term::string(COMMAND));
        }
        tell("push-log", Term::string(SHA), "landed-by", Term::string("n882-worker"));
        tell("git", Term::string(SHA), "ancestor-of", Term::string("origin/main"));
        path
    };
    let path = seeded("graph.db");
    let evidence = format!("Done when: `{COMMAND}`. Evidence: `{COMMAND}` passed on main 59ff4fb9c2d2 on 2026-11-08, exit 0.");
    let forged = format!("- [x] {box_text}. Done when: `{COMMAND}` exits 0. Evidence: `{COMMAND}` passed on main 59ff4fb9c2d2 on 2026-11-08, exit 0.\n- [x] {split_one}. {evidence}\n- [x] {split_two}. {evidence}\n");
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
    let corroborated = super::corroborated::checked_boxes(&path).unwrap();
    let named = |text: &str| super::closed::SpendRun { box_text: Some(text.into()), ..run.clone() };
    let items = closed.keys().cloned().collect();
    let report = super::summarize(&[named("Corroborated rewards"), named("Split short name")], &items, &Default::default(), "2026-11-01T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap()).verified(&closed, corroborated.as_ref());
    let graded = |text: &str| report.arms.iter().find(|arm| arm.box_text.as_deref() == Some(text)).map(|arm| arm.verified);
    assert_eq!(graded("Corroborated rewards"), Some(true), "a run naming its box by the leading words grades once the whole box is corroborated");
    assert_eq!(graded("Split short name"), Some(false), "a name that two boxes of its issue extend grades neither, though both are corroborated");
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

#[test]
fn one_repository_failing_on_github_leaves_the_other_reports_recorded_and_fails_the_unit_on_the_third_run() {
    use super::closed_source::{ClosedIssue, Inputs, Listing, report_between};
    use super::recorded::{Held, listed, settle};
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    let directory = tempfile::tempdir().unwrap();
    let runs = || vec![worker_run("north-worker", "north#5", "2026-11-08T10:00:00Z", "2026-11-08T11:00:00Z", true), worker_run("makgora-worker", "makgora#7", "2026-11-08T10:00:00Z", "2026-11-08T11:00:00Z", true)];
    let issue = |item: &str| ClosedIssue { item: item.into(), at: "2026-11-08T11:30:00Z".into(), closed: super::Closed::of_body(LEAF) };
    let prices = crate::router::pricing::PriceTable::load().unwrap();
    let record = |makgora_reads: bool| {
        let unread = Mutex::new(BTreeSet::new());
        let sources = Inputs { runs: runs(), native: Default::default(), closed: Default::default(), corroborated: None, catalog: &crate::threads::catalog::CATALOG };
        let github = |repo: &str, since: &str, end: &str| {
            let fetch = || match repo {
                "makgora" if !makgora_reads => Err(crate::error::NorthError::Configuration("gh issue list tompassarelli/makgora: Post https://api.github.com/graphql: EOF".into())),
                _ => Ok(Held { issues: vec![issue(&format!("{repo}#{}", if repo == "north" { 5 } else { 7 }))], open: Default::default() }),
            };
            let Held { issues, open } = listed(repo, fetch, &directory.path().join(format!("{repo}.json")), &unread);
            Ok(Listing { closed: super::closed_source::within(&issues, since, end), open })
        };
        let report = report_between(&sources, "2026-11-01T00:00:00Z", "2026-11-09T00:00:00Z", github, &prices).unwrap();
        (report.closed, unread.into_inner().unwrap().into_iter().collect::<Vec<_>>())
    };
    assert_eq!(record(true), (2, Vec::new()));
    assert_eq!(record(false), (2, vec!["makgora".to_owned()]), "the failed repository keeps its last good rows and is named");
    std::fs::remove_file(directory.path().join("makgora.json")).unwrap();
    assert_eq!(record(false), (1, vec!["makgora".to_owned()]), "with no last good rows the other repositories' report is still recorded");

    let counter = directory.path().join("unread-runs");
    let unread = || vec!["makgora".to_owned()];
    assert_eq!(settle(&counter, unread()).unwrap(), unread());
    assert_eq!(settle(&counter, unread()).unwrap(), unread());
    assert!(settle(&counter, unread()).unwrap_err().user_message().contains("makgora in 3 runs in a row"));
    assert!(settle(&counter, Vec::new()).unwrap().is_empty());
    assert_eq!(settle(&counter, unread()).unwrap(), unread(), "a read that succeeds restarts the count");
}

#[test]
fn a_worker_that_may_not_write_an_older_ledger_still_opens_and_reads_it() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(ledger(&directory));
    Connection::open(&path).unwrap().pragma_update(None, "user_version", super::SCHEMA_VERSION - 1).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err(), "the ledger stayed writable, as it does to root, so no worker domain was simulated");
    let reader = Ledger::open(&path).unwrap();
    reader.runs().unwrap();
}
