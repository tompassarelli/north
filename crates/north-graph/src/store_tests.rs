use std::sync::{Arc, Barrier};

use crate::store::{Action, Graph};
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
