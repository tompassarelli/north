use north_graph::{Graph, Term, Triple};

use crate::backlog::{Verdict, read_from};

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

fn graph() -> Graph {
    let mut graph = Graph::open_in_memory().unwrap();
    issue(&mut graph, "north#1", Some(1), &[("box:1a", "Ship the gateway"), ("box:1b", "Move the callers")]);
    tell(&mut graph, "box:1b", "waits-on", Term::string("box:2a"));
    issue(&mut graph, "north#2", Some(2), &[("box:2a", "Build the broker")]);
    issue(&mut graph, "north#3", None, &[("box:3a", "Cull")]);
    tell(&mut graph, "north#3", "blocked-by", Term::string("north#2"));
    issue(&mut graph, "north#4", Some(3), &[("box:4a", "Doc box"), ("box:4c", "Clean-up: scaffolding removed.")]);
    tell(&mut graph, "box:4a", "waits-on", Term::string("north#2"));
    issue(&mut graph, "north#5", Some(4), &[("box:5a", "Waits on a closed issue")]);
    tell(&mut graph, "north#5", "doc-waits-on", Term::string("north#6"));
    tell(&mut graph, "north#6", "state", Term::keyword("closed"));
    graph
}

#[test]
fn the_reader_lists_ready_boxes_in_rank_order_with_each_held_boxs_edge_from_the_layout_ingest_stores() {
    let backlog = read_from(&graph()).unwrap();
    let ready: Vec<(&str, &str)> = backlog.ready.iter().map(|entry| (entry.issue.as_str(), entry.node.as_str())).collect();
    assert_eq!(ready, [("north#1", "box:1a"), ("north#2", "box:2a"), ("north#4", "box:4c"), ("north#5", "box:5a")]);
    let held: Vec<(&str, &str, &str)> = backlog.held.iter().map(|held| (held.entry.issue.as_str(), held.entry.node.as_str(), held.edge.as_str())).collect();
    assert_eq!(held, [("north#1", "box:1b", "box:2a"), ("north#3", "box:3a", "north#2"), ("north#4", "box:4a", "north#2")]);
    assert_eq!(backlog.held[0].edge_text, "Build the broker", "a box edge reads as the box it waits on");
    assert_eq!(backlog.held[1].edge_text, "title of north#2", "an issue edge reads as the issue's title");
    assert_eq!((backlog.rank("north#2"), backlog.rank("north#3")), (Some(2), None));

    assert!(matches!(backlog.verdict("north#1"), Verdict::Ready), "box 1a is ready while 1b waits");
    assert!(matches!(backlog.verdict("north#5"), Verdict::Ready), "a wait on a closed issue holds nothing");
    let Verdict::Held(why) = backlog.verdict("north#3") else { panic!("north#3 waits on open north#2") };
    assert_eq!(why, "box \"Cull\" waits on north#2 \"title of north#2\"");
    assert!(matches!(backlog.verdict("north#4"), Verdict::Held(_)), "a ready Clean-up box does not free an issue whose product box waits");
    assert!(matches!(backlog.verdict("north#99"), Verdict::Unknown));

    let lines = backlog.lines();
    assert_eq!(lines[0], "ready 1\tnorth#1\trank 1\tShip the gateway");
    assert_eq!(lines[2], "ready 3\tnorth#5\trank 4\tWaits on a closed issue", "north#4's ready Clean-up box is not listed while a product box of it waits");
    assert_eq!(lines[3], "held\tnorth#1\trank 1\tMove the callers\twaits on box:2a \"Build the broker\"");
}
