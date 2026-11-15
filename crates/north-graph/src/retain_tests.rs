use crate::store::{Action, Graph, Reach};
use crate::term::{Term, Triple};

fn edge(from: &str, to: i64) -> Triple {
    Triple::new(from, Term::keyword("edge"), to)
}

#[test]
fn retain_keeps_a_term_a_writer_references_again_between_its_snapshot_and_its_delete() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.apply("mirror", &[Action::assert(edge("a", 1)), Action::retract(edge("a", 1))]).unwrap();
    graph.connection().execute_batch("DELETE FROM withdrawals; DELETE FROM occurrences;").unwrap();
    let mut reach = Reach::snapshot(graph.connection()).unwrap();
    let candidates = reach.candidates(graph.connection()).unwrap();
    assert!(!candidates.is_empty());
    graph.tell("mirror", Triple::new("claim", Term::keyword("quotes"), Term::from(edge("a", 1)))).unwrap();
    reach.delete(graph.connection(), &candidates).unwrap();
    let report = graph.validate().unwrap();
    assert!(report.is_sound(), "{:?}", report.problems);
    assert_eq!(graph.show(&Term::string("claim")).unwrap().len(), 1);
}
