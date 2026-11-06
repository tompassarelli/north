use crate::store::{Action, Graph};
use crate::term::{Term, Triple};

fn fact(writer: usize, index: usize) -> Triple {
    Triple::new(Term::string(format!("writer-{writer}")), Term::keyword("wrote"), i64::try_from(index).unwrap())
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
