use std::path::Path;

use north_graph::{Graph, Query, Term, parse_query};
use north_github_mirror::store::Mirror;
use rusqlite::Connection;
use serde_json::json;

use crate::{Places, Source, ingest};

const FULL: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
const OTHER: &str = "0aa2d15297f1a6a2be5ba41e5bbf4c8a9d3a2f10";
const HUNG: &str = "box:north#634:a-hung-slot-job-is-killed-done-when-cargo-test-p";

fn body(first_ticked: bool) -> String {
    let mark = if first_ticked { "x" } else { " " };
    format!(
        "- [{mark}] A hung slot job is killed. Done when: `cargo test -p slots` exits 0 (owner: n634-hung-slots; landed 59ff4fb9c)\n- [x] The gate stops waiting (owner: n634-no-laptop-cargo; landed 0aa2d1529)\n- [x] Nobody ran this one (owner: n634-ghost)\n- [ ] Open work (owner: unassigned)\n"
    )
}

fn tick(mirror: &Mirror, first_ticked: bool) {
    let issue = json!({"number": 634, "title": "Verification", "body": body(first_ticked), "labels": {"nodes": [{"name": "bet"}]}, "blockedBy": {"nodes": []}});
    mirror.write("north/issues/open", &json!([issue]), None, 1.0).unwrap();
    mirror.write("north/issues/closed", &json!({"numbers": [7]}), None, 1.0).unwrap();
}

fn places(directory: &Path) -> Places {
    let mirror = Mirror::at(directory.join("mirror.db"));
    tick(&mirror, true);
    let ledger = directory.join("ledger.db");
    Connection::open(&ledger)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE checks(check_id INTEGER PRIMARY KEY, task_ref TEXT NOT NULL, command TEXT NOT NULL, cwd TEXT NOT NULL, head_sha TEXT, exit INTEGER NOT NULL, ran_ms INTEGER NOT NULL);
             CREATE TABLE tasks(task_ref TEXT PRIMARY KEY, category TEXT, sessions TEXT NOT NULL DEFAULT '[]', wall_ms INTEGER, outcome TEXT, landed_sha TEXT, review TEXT, corrections INTEGER NOT NULL DEFAULT 0, landed_ms INTEGER);
             INSERT INTO checks VALUES (1, 'tompassarelli/north#634', 'safe-push landing', '', '{FULL}', 0, 1);
             INSERT INTO tasks (task_ref, sessions, landed_sha) VALUES ('north#634', '[\"n634-hung-slots\"]', '{OTHER}');"
        ))
        .unwrap();
    std::fs::write(directory.join("pushes.tsv"), format!("1793926432\tnorth\tn634-hung-slots\t{FULL}\t{FULL}\tn634-hung-slots\n")).unwrap();
    std::fs::write(directory.join("corrections.jsonl"), "{\"id\":\"L1\",\"at\":\"2026-10-01T14:15:12Z\",\"kind\":\"correction\",\"quote\":\"Tom said the reply did not answer his question.\",\"source\":\"tom convo:1 session:abc\",\"superseded_by\":null,\"class\":\"unanswered\"}\n").unwrap();
    std::fs::create_dir_all(directory.join("coverage")).unwrap();
    Places { mirror, ledger, threads: directory.join("threads.db"), pushes: directory.join("pushes.tsv"), code: directory.join("code"), corrections: directory.join("corrections.jsonl"), coverage: directory.join("coverage"), ranking: directory.join("backlog-ranking.md"), land_reuse: directory.join("land-reuse"), verdict_cache: directory.join("verdict-cache") }
}

fn unobserved_claims() -> Query {
    let claim = json!({"head": {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]}, "body": [
        {"rel": "assertion", "args": [{"var": "who"}, {"var": "speaker"}, ["keyword", "claims"], {"var": "q"}]},
        {"pred": "eq", "args": [{"var": "who"}, {"var": "speaker"}]},
        {"rel": "quoted", "args": [{"var": "q"}, {"var": "box"}, ["keyword", "done-at"], {"var": "sha"}]},
        {"rel": "triple", "args": ["north#634", ["keyword", "has-box"], {"var": "box"}]}]});
    let observed = json!({"head": {"rel": "observed", "args": [{"var": "sha"}]}, "body": [
        {"rel": "assertion", "args": ["checker", {"var": "sha"}, ["keyword", "check-passed"], {"var": "command"}]}]});
    let unobserved = json!({"head": {"rel": "unobserved", "args": [{"var": "who"}, {"var": "sha"}]}, "body": [
        {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]},
        {"rel": "observed", "args": [{"var": "sha"}], "neg": true}]});
    parse_query(&json!({"find": "unobserved", "strata": [[claim, observed], [unobserved]]}).to_string()).unwrap()
}

fn unobserved(graph: &Graph) -> Vec<(Term, Term)> {
    graph.ask(&unobserved_claims()).unwrap().rows.into_iter().map(|row| (row[0].clone(), row[1].clone())).collect()
}

#[test]
fn a_done_claim_is_the_workers_quoted_triple_only_the_checker_can_observe_and_a_rerun_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Mirror, Source::Ledger, Source::Pushes]);
    assert!(!report.failed, "{:?}", report.lines);
    let mut found = unobserved(&graph);
    found.sort();
    assert_eq!(
        found,
        [(Term::string("n634-ghost"), Term::keyword("none")), (Term::string("n634-no-laptop-cargo"), Term::string(OTHER))],
        "the claim whose commit the checker passed is observed; a claim naming no commit, and one the checker never saw, are not"
    );
    let told = graph.told().unwrap();
    assert!(
        !told.iter().any(|fact| fact.triple.t2 == Term::keyword("done-at")),
        "the commit a box names is quoted inside the worker's claim and is never a bare fact"
    );
    a_run_claims_its_box_of_a_closed_issue_and_the_lander_observes_the_landing(&places, &mut graph);
    a_fleet_session_cannot_tell_under_a_trusted_name();
    a_rerun_adds_nothing_and_a_changed_source_is_followed();
}

fn a_fleet_session_cannot_tell_under_a_trusted_name() {
    use crate::trusted_writers::refusal;
    let session = Some("0::/user.slice/north-session-7.scope/inner\n");
    let broker = Some("0::/user.slice/north-worker-outside-1.service\n");
    for writer in ["checker", "mirror", "lander", "git"] {
        assert!(refusal("tell", writer, session).is_some() && refusal("retract", writer, broker).is_some(), "{writer}");
    }
    assert!(refusal("tell", "checker", Some("0::/user.slice/north-lander.service\n")).is_none() && refusal("tell", "checker", None).is_some());
    let held = Some("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-fleet-north-1547-d1752-graded-3730d742.service\n");
    assert!(refusal("tell", "checker", held).is_some() && refusal("retract", "lander", held).is_some(), "a held session's unit is a fleet session's scope");
    assert!(refusal("show", "checker", session).is_none() && refusal("tell", "selection-miss", session).is_none());
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("graph.db").display().to_string();
    let words = ["tell", "--db", &database, "--writer", "checker", "--file", "--writer", "S", "check-passed"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| refusal(verb, writer, session));
    assert_eq!(code, 1, "the writer the parser resolved is the one refused, since --file takes the later --writer as its value");
    assert!(!directory.path().join("graph.db").exists());
    let seen = std::cell::RefCell::new(Vec::new());
    let words = ["tell", "--db", &database, "--writer", "S", "--file", "--writer", "checker"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| {
        seen.borrow_mut().push(writer.to_owned());
        refusal(verb, writer, session)
    });
    assert_eq!(*seen.borrow(), ["S"], "the parser resolves S, so a reading of the raw words that refuses checker here is the wrong one");
    assert_eq!(code, 2, "the guard let it through and the write failed on its arguments");
    use crate::trusted_writers::ingest_refusal;
    let collector = Some("0::/user.slice/north-lander.service\n");
    for source in [Source::Mirror, Source::Ledger, Source::Touches, Source::Observations] {
        assert!(ingest_refusal(source.name(), source.writers(), session).is_some() && ingest_refusal(source.name(), source.writers(), None).is_some(), "{}", source.name());
    }
    for source in [Source::Ranking, Source::Losses, Source::Pushes, Source::Corrections, Source::Coverage, Source::Crates, Source::Reads] {
        assert!(ingest_refusal(source.name(), source.writers(), session).is_none(), "{}", source.name());
    }
    assert!(Source::ALL.into_iter().all(|source| ingest_refusal(source.name(), source.writers(), collector).is_none()));
}

fn a_run_claims_its_box_of_a_closed_issue_and_the_lander_observes_the_landing(places: &Places, graph: &mut Graph) {
    Connection::open(&places.threads)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE runs(agent TEXT, item TEXT, box TEXT, landed_sha TEXT, outcome TEXT, role TEXT, category TEXT, brief_text TEXT, minutes REAL);
             INSERT INTO runs VALUES ('run-7', 'north#7', 'Ship the closed box. Done when: `cargo test -p x` exits 0', '{FULL}', 'done', 'worker', 'feature', '', 30);
             INSERT INTO runs VALUES ('run-634', 'north#634', 'A hung slot job is killed', '{FULL}', 'done', 'worker', 'feature', '', 30);"
        ))
        .unwrap();
    let report = ingest(graph, places, &[Source::Mirror, Source::Observations]);
    assert!(!report.failed, "{:?}", report.lines);
    let node = Term::string("box:north#7:ship-the-closed-box-done-when-cargo-test-p-x-exi");
    let told = graph.told().unwrap();
    assert!(told.contains(&north_graph::Told::new("run-7", "run-7", Term::keyword("claims"), Term::triple(node.clone(), Term::keyword("done-at"), Term::string(FULL)))), "a done run claims its box though the issue is closed: {:?}", report.lines);
    assert!(told.contains(&north_graph::Told::new("ledger", node, Term::keyword("done-when"), Term::string("cargo test -p x"))));
    assert!(told.contains(&north_graph::Told::new("lander", FULL, Term::keyword("landed-on"), Term::string("main"))), "{:?}", report.lines);
    let again = ingest(graph, places, &[Source::Observations]);
    assert!(again.lines.iter().any(|line| line.starts_with("observations: 0 asserted")), "{:?}", again.lines);
    let hung = Term::string(HUNG);
    assert!(told.contains(&north_graph::Told::new("run-634", "run-634", Term::keyword("claims"), Term::triple(hung.clone(), Term::keyword("done-at"), Term::string(FULL)))), "a claim under the first words of a box is told on the whole box");
    places.mirror.write("north/issues/open", &json!([]), None, 2.0).unwrap();
    places.mirror.write("north/issues/closed", &json!({"numbers": [7, 634]}), None, 2.0).unwrap();
    let gone = north_graph::Told::new("mirror", "box:north#9:gone", Term::keyword("ticked"), Term::boolean(false));
    let intruder = north_graph::Told::new("session-9", "north#634", Term::keyword("has-box"), Term::string("box:north#9:gone"));
    let unbacked = north_graph::Told::new("session-9", "session-9", Term::keyword("claims"), Term::triple(Term::string(HUNG), Term::keyword("done-at"), Term::string(FULL)));
    for fact in [&gone, &intruder, &unbacked] {
        graph.tell(&fact.writer, fact.triple.clone()).unwrap();
    }
    let closed = ingest(graph, places, &[Source::Mirror]);
    let told = graph.told().unwrap();
    assert!(!told.contains(&gone) && !told.contains(&intruder) && !told.contains(&unbacked), "only the mirror's own facts of a closed issue are held, so another writer's has-box neither stays nor keeps a stale mirror fact of its object, and a claim stays only while a run still backs it");
    assert!(told.contains(&north_graph::Told::new("run-634", "run-634", Term::keyword("claims"), Term::triple(Term::string(HUNG), Term::keyword("done-at"), Term::string(FULL)))), "a run's claim on a box of an issue that closed stays on the whole box: {:?}", closed.lines);
    for (subject, predicate, object) in [("north#634", "has-box", hung), (HUNG, "done-when", Term::string("cargo test -p slots")), ("north#634", "state", Term::keyword("closed"))] {
        assert!(told.contains(&north_graph::Told::new("mirror", subject, Term::keyword(predicate), object)), "a closed issue keeps the box facts it had while open: {subject} {predicate} {:?}", closed.lines);
    }
}

fn a_rerun_adds_nothing_and_a_changed_source_is_followed() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let mut graph = Graph::open_in_memory().unwrap();
    let all = [Source::Mirror, Source::Ledger, Source::Pushes, Source::Corrections];
    ingest(&mut graph, &places, &all);
    let before = graph.validate().unwrap().occurrences;
    let again = ingest(&mut graph, &places, &all);
    assert_eq!(graph.validate().unwrap().occurrences, before, "{:?}", again.lines);
    assert!(again.lines.iter().all(|line| line.contains("0 asserted, 0 retracted")), "{:?}", again.lines);
    let claims = |graph: &Graph| graph.told().unwrap().iter().filter(|fact| fact.triple.t2 == Term::keyword("claims")).count();
    assert_eq!(claims(&graph), 3);
    tick(&places.mirror, false);
    ingest(&mut graph, &places, &all);
    assert_eq!(claims(&graph), 2, "unticking the box withdraws its owner's claim");
    assert!(graph.validate().unwrap().is_sound());
}

