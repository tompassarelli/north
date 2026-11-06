use std::path::Path;

use north_graph::{Graph, Query, Term, parse_query};
use north_github_mirror::store::Mirror;
use rusqlite::Connection;
use serde_json::json;

use crate::{Places, Source, ingest};

const FULL: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
const OTHER: &str = "0aa2d15297f1a6a2be5ba41e5bbf4c8a9d3a2f10";

fn body(first_ticked: bool) -> String {
    let mark = if first_ticked { "x" } else { " " };
    format!(
        "- [{mark}] A hung slot job is killed (owner: n634-hung-slots; landed 59ff4fb9c)\n- [x] The gate stops waiting (owner: n634-no-laptop-cargo; landed 0aa2d1529)\n- [x] Nobody ran this one (owner: n634-ghost)\n- [ ] Open work (owner: unassigned)\n"
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
    Places { mirror, ledger, pushes: directory.join("pushes.tsv"), code: directory.join("code"), decisions: None, corrections: directory.join("corrections.jsonl"), coverage: directory.join("coverage") }
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
fn a_done_claim_is_the_workers_quoted_triple_and_only_the_checker_can_observe_it() {
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
}

#[test]
fn ingesting_twice_adds_nothing_and_a_changed_source_is_followed() {
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

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.com"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_landed_commit_is_told_to_have_touched_the_files_it_changed() {
    let directory = tempfile::tempdir().unwrap();
    let mut places = places(directory.path());
    let checkout = directory.path().join("code/north/main");
    std::fs::create_dir_all(checkout.join("crates/x")).unwrap();
    git(&checkout, &["init", "-q"]);
    std::fs::write(checkout.join("crates/x/lib.rs"), "a").unwrap();
    std::fs::write(checkout.join("README.md"), "a").unwrap();
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "one"]);
    let landed = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("README.md"), "b").unwrap();
    git(&checkout, &["commit", "-q", "-am", "never pushed"]);
    let now = north_base::foundation::clock::Timestamp::now().unix_seconds();
    std::fs::write(&places.pushes, format!("{now}\tnorth\tlane\t{landed}\t{landed}\tn669-grounding\n")).unwrap();
    places.code = directory.path().join("code");
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Touches]);
    assert!(!report.failed, "{:?}", report.lines);
    let mut touched: Vec<String> = graph.mentions(&Term::string(landed.as_str()), 10).unwrap().into_iter().filter(|fact| fact.triple.t2 == Term::keyword("touched")).map(|fact| fact.triple.t3.to_string()).collect();
    touched.sort();
    assert_eq!(touched, ["\"README.md\"", "\"crates/x/lib.rs\""], "only a commit the push log landed is read, with the files that commit changed");
}
