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
    issue(&mut graph, "north#4", Some(3), &[("box:4a", "Doc box")]);
    tell(&mut graph, "box:4a", "waits-on", Term::string("north#2"));
    issue(&mut graph, "north#5", Some(4), &[("box:5a", "Waits on a closed issue")]);
    tell(&mut graph, "north#5", "doc-waits-on", Term::string("north#6"));
    tell(&mut graph, "north#6", "state", Term::keyword("closed"));
    tell(&mut graph, "north#2", "label", Term::string("priority:later"));
    tell(&mut graph, "north#5", "loss-usd", Term::float(9.0));
    tell(&mut graph, "north#5", "box-usd", Term::float(1.5));
    graph
}

#[test]
fn the_reader_lists_ready_boxes_most_measured_loss_per_dollar_first_then_by_label_and_rank_with_each_held_boxs_edge_and_holds_a_box_behind_its_dated_check() {
    let backlog = read_from(&graph()).unwrap();
    let ready: Vec<(&str, &str)> = backlog.ready.iter().map(|entry| (entry.issue.as_str(), entry.node.as_str())).collect();
    assert_eq!(ready, [("north#5", "box:5a"), ("north#1", "box:1a"), ("north#2", "box:2a")], "the issue owning measured loss leads, and a later label sinks below rank");
    assert_eq!(backlog.leaders(), ["north#5", "north#1", "north#2"]);
    let held: Vec<(&str, &str, &str)> = backlog.held.iter().map(|held| (held.entry.issue.as_str(), held.entry.node.as_str(), held.edge.as_str())).collect();
    assert_eq!(held, [("north#1", "box:1b", "box:2a"), ("north#3", "box:3a", "north#2"), ("north#4", "box:4a", "north#2")]);
    assert_eq!(backlog.held[0].edge_text, "Build the broker", "a box edge reads as the box it waits on");
    assert_eq!(backlog.held[1].edge_text, "title of north#2", "an issue edge reads as the issue's title");
    assert_eq!((backlog.rank("north#2"), backlog.rank("north#3")), (Some(2), None));

    assert!(matches!(backlog.verdict("north#1"), Verdict::Ready), "box 1a is ready while 1b waits");
    assert!(matches!(backlog.verdict("north#5"), Verdict::Ready), "a wait on a closed issue holds nothing");
    let Verdict::Held(why) = backlog.verdict("north#3") else { panic!("north#3 waits on open north#2") };
    assert_eq!(why, "box \"Cull\" waits on north#2 \"title of north#2\"");
    assert!(matches!(backlog.verdict("north#4"), Verdict::Held(_)), "box 4a waits on open north#2");
    assert!(matches!(backlog.verdict("north#99"), Verdict::Unknown));

    let lines = backlog.lines();
    assert_eq!(lines[0], "ready 1\tnorth#5\t6.0 = $9.00 lost 24 h / $1.50 a box, rank 4\tWaits on a closed issue");
    assert_eq!(lines[2], "ready 3\tnorth#2\t0 measured, rank 2\tBuild the broker");
    assert_eq!(lines[3], "held\tnorth#1\t0 measured, rank 1\tMove the callers\twaits on box:2a \"Build the broker\"");

    let mut graph = graph();
    issue(&mut graph, "north#706", Some(5), &[("box:706a", "Over 3 h no run is attributed host-caused (not before 2026-11-08T00:00Z)")]);
    tell(&mut graph, "north#706", "loss-usd", Term::float(90.0));
    tell(&mut graph, "north#706", "box-usd", Term::float(1.0));
    let before = crate::backlog::read_from_at(&graph, 1_794_017_760).unwrap();
    assert_eq!(before.leaders(), ["north#5", "north#1", "north#2"], "north#706 led by measured loss on 2026-11-07 with nothing takeable");
    let Verdict::Held(why) = before.verdict("north#706") else { panic!("north#706 waits for its date") };
    assert_eq!(why, "box \"Over 3 h no run is attributed host-caused (not before 2026-1…\" waits on its dated check \"2026-11-08T00:00Z\"");
    assert_eq!(crate::backlog::read_from_at(&graph, 1_794_096_000).unwrap().leaders()[0], "north#706", "it leads again once its date comes");
}
