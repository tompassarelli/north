use north_graph::{Graph, Term, Triple};

use crate::backlog::read_from;

fn tell(graph: &mut Graph, subject: &str, predicate: &str, object: impl Into<Term>) {
    graph.tell("mirror", Triple::new(Term::string(subject), Term::keyword(predicate), object.into())).unwrap();
}

fn issue(graph: &mut Graph, name: &str, rank: Option<i64>, boxes: &[(&str, &str)]) {
    tell(graph, name, "state", Term::keyword("open"));
    tell(graph, name, "title", Term::string(format!("title of {name}")));
    if let Some(rank) = rank {
        tell(graph, name, "rank", Term::int(rank));
    }
    for (node, text) in boxes {
        tell(graph, name, "has-box", Term::string(*node));
        tell(graph, node, "text", Term::string(*text));
        tell(graph, node, "ticked", Term::boolean(false));
    }
}

#[test]
fn a_red_main_leads_priority_now_and_unranked_ties_go_oldest_first_then_by_number_with_a_child_taking_its_parents_label() {
    let mut graph = Graph::open_in_memory().unwrap();
    for (name, created) in [("firn#41", 300), ("makgora#1122", 100), ("makgora#889", 100), ("north#1495", 200), ("north#1537", 400), ("north#1550", 500)] {
        issue(&mut graph, name, None, &[(format!("box:{name}").as_str(), "Do it")]);
        tell(&mut graph, name, "created", Term::int(created));
    }
    tell(&mut graph, "north#1537", "label", Term::string("main-red"));
    tell(&mut graph, "north#1495", "label", Term::string("priority:now"));
    tell(&mut graph, "north#1550", "parent", Term::string("north#1495"));
    let backlog = read_from(&graph).unwrap();
    assert_eq!(backlog.leaders(), ["north#1537", "north#1495", "north#1550", "makgora#889", "makgora#1122", "firn#41"], "on 2026-11-14 string order put firn#41 and makgora#1122 first");
}
