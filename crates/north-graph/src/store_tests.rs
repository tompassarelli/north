use std::collections::HashSet;
use std::sync::{Arc, Barrier};

use crate::store::{Action, Graph, Told};
use crate::sync::{Synced, Telling};
use crate::term::{Term, Triple};

fn fact(writer: usize, index: usize) -> Triple {
    Triple::new(Term::string(format!("writer-{writer}")), Term::keyword("wrote"), i64::try_from(index).unwrap())
}

#[test]
fn concurrent_writers_each_keep_every_acknowledged_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    Graph::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|writer| {
            let (path, barrier) = (path.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut graph = Graph::open(&path).unwrap();
                barrier.wait();
                for index in 0..25 {
                    graph.tell(&format!("writer-{writer}"), fact(writer, index)).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let graph = Graph::open(&path).unwrap();
    let report = graph.validate().unwrap();
    assert_eq!(report.occurrences, 200);
    assert_eq!(report.live_triples, 200);
    assert!(report.is_sound(), "{:?}", report.problems);
    for writer in 0..8 {
        assert_eq!(graph.show(&Term::string(format!("writer-{writer}"))).unwrap().len(), 25);
    }
}

#[test]
fn what_one_process_wrote_a_later_open_reads_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/graph.db");
    Graph::open(&path).unwrap().apply("a", &[Action::assert(fact(0, 1)), Action::retract(fact(0, 2))]).unwrap();
    let again = Graph::open(&path).unwrap();
    assert_eq!(again.show(&Term::string("writer-0")).unwrap().len(), 1);
    let report = again.validate().unwrap();
    assert_eq!((report.occurrences, report.live_occurrences), (2, 1));
}

#[test]
fn validate_reports_a_store_whose_live_state_disagrees_with_its_history() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.tell("a", fact(0, 1)).unwrap();
    graph.tell("a", fact(0, 2)).unwrap();
    graph.retract("a", fact(0, 1)).unwrap();
    assert!(graph.validate().unwrap().is_sound());
    graph.connection().execute("UPDATE occurrences SET live = 1 WHERE seq = 1", []).unwrap();
    assert!(!graph.validate().unwrap().is_sound(), "a withdrawn assertion marked live must be reported");
    graph.connection().execute("UPDATE occurrences SET live = 0 WHERE seq = 1", []).unwrap();
    graph.connection().execute("DELETE FROM live_props", []).unwrap();
    assert!(!graph.validate().unwrap().is_sound(), "a live assertion missing from the triple relation must be reported");
}

#[test]
fn a_batch_is_one_transaction_that_commits_whole_or_not_at_all() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.connection().execute("CREATE TRIGGER refuse AFTER INSERT ON occurrences WHEN NEW.seq = 3 BEGIN SELECT RAISE(ABORT, 'refused'); END", []).unwrap();
    let batch = [Action::assert(fact(0, 1)), Action::assert(fact(0, 2)), Action::assert(fact(0, 3))];
    assert!(graph.apply("a", &batch).is_err());
    assert_eq!(graph.validate().unwrap().occurrences, 0, "the first two actions were rolled back with the third");
    assert!(graph.show(&Term::string("writer-0")).unwrap().is_empty());
}

#[test]
fn validate_reports_an_occurrence_whose_coordinate_term_is_missing_instead_of_failing() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.tell("a", fact(0, 1)).unwrap();
    graph.connection().execute("UPDATE occurrences SET coord = 999999 WHERE seq = 1", []).unwrap();
    let report = graph.validate().unwrap();
    assert!(!report.is_sound());
}

#[test]
fn a_sync_reads_only_replaced_predicates_and_its_own_writers_facts() {
    let mut graph = Graph::open_in_memory().unwrap();
    let told = |writer: &str, t1: &str, t2: &str, t3: &str| Told::new(writer, Term::string(t1), Term::keyword(t2), Term::string(t3));
    for fact in [told("mirror", "a", "title", "x"), told("other", "b", "note", "y"), told("other", "c", "title", "z"), told("ledger", "d", "at", "1"), told("other", "e", "at", "2")] {
        graph.tell(&fact.writer, fact.triple).unwrap();
    }
    let title = Term::keyword("title");
    let at = Term::keyword("at");
    let mut scoped = graph.told_within(&HashSet::from([("ledger", &at)]), &HashSet::from([&title])).unwrap();
    scoped.sort_by_key(|told| told.triple.t1.to_string());
    assert_eq!(scoped, [told("mirror", "a", "title", "x"), told("other", "c", "title", "z"), told("ledger", "d", "at", "1")]);
    let synced = graph.sync(&Telling { facts: vec![told("mirror", "a", "title", "x"), told("ledger", "d", "at", "1")], replaces: vec![title] }).unwrap();
    assert_eq!(synced, Synced { asserted: 0, retracted: 1, unchanged: 2 });
    let mut left: Vec<String> = graph.told().unwrap().iter().map(|told| told.triple.t1.to_string()).collect();
    left.sort();
    assert_eq!(left, ["\"a\"", "\"b\"", "\"d\"", "\"e\""]);
}

#[test]
fn retiring_a_writer_withdraws_each_of_its_live_assertions_and_keeps_the_rest() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.tell("decision-log", fact(0, 1)).unwrap();
    graph.tell("decision-log", fact(0, 1)).unwrap();
    graph.tell("decision-log", fact(0, 2)).unwrap();
    graph.tell("mirror", fact(1, 1)).unwrap();
    assert_eq!(graph.retire("decision-log").unwrap(), 3);
    assert_eq!(graph.retire("decision-log").unwrap(), 0);
    let report = graph.validate().unwrap();
    assert!(report.is_sound());
    assert_eq!((report.occurrences, report.live_occurrences), (7, 1));
    assert_eq!(graph.told().unwrap(), [Told { writer: "mirror".into(), triple: fact(1, 1) }]);
}

// KEEP(bug): on 2026-11-08 n882-textrel (5b1623c4f6) raised the graph schema to 4 and its after-landing run went red: the graph guards check, a read-only open, called the build host's schema-2 graph.db corrupt, though a writable open upgrades that store in place (north#902)
#[test]
fn a_read_only_open_upgrades_a_store_an_older_build_wrote_instead_of_calling_it_corrupt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    let mut graph = Graph::open(&path).unwrap();
    graph.tell("writer-0", fact(0, 1)).unwrap();
    let head = graph.head().unwrap();
    graph
        .connection()
        .execute_batch("CREATE TABLE terms_v2 (id INTEGER PRIMARY KEY, key BLOB NOT NULL UNIQUE); INSERT INTO terms_v2 SELECT id, key FROM terms; DROP TABLE terms; ALTER TABLE terms_v2 RENAME TO terms; PRAGMA user_version = 2;")
        .unwrap();
    drop(graph);
    let reader = Graph::open_read_only(&path, std::time::Duration::from_secs(1)).unwrap();
    assert_eq!(reader.head().unwrap(), head, "the upgraded store keeps every fact the older build told");
}
