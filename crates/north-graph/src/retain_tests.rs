use serde_json::{Value, json};

use crate::error::{GraphError, QueryError};
use crate::query::parse_query;
use crate::store::{Action, Graph, Reach, Retention};
use crate::term::{Term, Triple};

const DAY: i64 = 86_400;
const MARKED_AT: i64 = 1_800_000_000;

fn edge(from: &str, to: i64) -> Triple {
    Triple::new(from, Term::keyword("edge"), to)
}

fn titled(issue: &str, title: &str) -> Triple {
    Triple::new(issue, Term::keyword("title"), Term::string(title))
}

fn logged(index: i64) -> Triple {
    Triple::new("decision", Term::keyword("logged"), index)
}

fn head(graph: &Graph) -> i64 {
    graph.connection().query_row("SELECT MAX(seq) FROM occurrences", [], |row| row.get(0)).unwrap()
}

fn edges(graph: &Graph, relation: &str, view: Option<(&str, Value)>) -> Result<Vec<String>, GraphError> {
    let (args, head) = match relation {
        "assertion" => (json!([{"var": "w"}, {"var": "a"}, ["keyword", "edge"], {"var": "b"}]), json!([{"var": "w"}, {"var": "a"}, {"var": "b"}])),
        _ => (json!([{"var": "a"}, ["keyword", "edge"], {"var": "b"}]), json!([{"var": "a"}, {"var": "b"}])),
    };
    let mut query = json!({"find": "seen", "rules": [{"head": {"rel": "seen", "args": head}, "body": [{"rel": relation, "args": args}]}]});
    if let Some((selector, bound)) = view {
        query[selector] = bound;
    }
    let rows = graph.ask(&parse_query(&query.to_string())?)?.rows;
    let mut rows: Vec<String> = rows.iter().map(|row| row.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")).collect();
    rows.sort();
    Ok(rows)
}

fn views(graph: &Graph, from: i64, to: i64) -> Vec<Vec<String>> {
    let mut seen = vec![edges(graph, "triple", None).unwrap(), edges(graph, "assertion", None).unwrap()];
    for cut in from..=to {
        seen.push(edges(graph, "triple", Some(("asOf", json!(cut)))).unwrap());
        seen.push(edges(graph, "assertion", Some(("asOf", json!(cut)))).unwrap());
        seen.push(edges(graph, "triple", Some(("since", json!([from, cut])))).unwrap());
    }
    seen
}

#[test]
fn retain_drops_retired_and_old_withdrawn_history_and_keeps_every_view_from_its_horizon() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.apply("decision-log", &[Action::assert(logged(1)), Action::assert(logged(2)), Action::retract(logged(1)), Action::retract(logged(2))]).unwrap();
    graph.apply("mirror", &[Action::assert(edge("a", 1)), Action::assert(edge("b", 2)), Action::assert(edge("c", 3))]).unwrap();
    graph.apply("mirror", &[Action::assert(titled("north#1", "landing queue")), Action::assert(titled("north#2", "lander probes")), Action::retract(titled("north#2", "lander probes"))]).unwrap();
    graph.apply("mirror", &[Action::retract(edge("a", 1)), Action::retract(edge("z", 9)), Action::assert(edge("y", 7))]).unwrap();
    graph.mark(MARKED_AT).unwrap();
    let horizon = head(&graph);
    graph.apply("mirror", &[Action::assert(edge("d", 4)), Action::retract(edge("b", 2)), Action::retract(edge("d", 4)), Action::assert(edge("a", 1))]).unwrap();
    graph.apply("decision-log", &[Action::assert(logged(3)), Action::retract(logged(3))]).unwrap();
    let last = head(&graph);
    let before = views(&graph, horizon, last);

    let retained = graph.retain(&Retention { retired: vec!["decision-log".into()], horizon_days: 45, now: MARKED_AT + 46 * DAY }).unwrap();

    assert_eq!(retained.horizon, Some(horizon));
    assert!(retained.full_vacuum, "the first run switches the store to incremental vacuum");
    assert_eq!(retained.occurrences, 9, "decision-log's first 4, the a-1 pair, the north#2 title pair and the failed z-9 retraction below the horizon");
    assert_eq!([graph.text_rows().unwrap(), graph.text_matches("land*").unwrap()], [1, 1], "the text index keeps the live title and no row of a deleted one");
    assert_eq!(views(&graph, horizon, last), before);
    let logs = graph.ask(&parse_query(r#"{"find":"l","rules":[{"head":{"rel":"l","args":[{"var":"x"}]},"body":[{"rel":"quoted","args":[{"var":"q"},"decision",["keyword","logged"],{"var":"x"}]}]}]}"#).unwrap()).unwrap();
    assert_eq!(logs.rows, vec![vec![Term::int(3)]], "only the head's pair of the retired writer is kept, and no term of the rest");
    for view in [("asOf", json!(horizon - 1)), ("since", json!([horizon - 1, last]))] {
        let refused = edges(&graph, "triple", Some(view.clone()));
        assert!(matches!(refused, Err(GraphError::Query(QueryError::SnapshotExpired { horizon: h, .. })) if h == horizon), "{view:?}: {refused:?}");
    }
    let report = graph.validate().unwrap();
    assert!(report.is_sound(), "{:?}", report.problems);
    assert_eq!(report.horizon, Some(horizon));
    let again = graph.retain(&Retention { retired: vec!["decision-log".into()], horizon_days: 45, now: MARKED_AT + 47 * DAY }).unwrap();
    assert_eq!((again.occurrences, again.terms, again.full_vacuum), (0, 0, false));
    graph.tell("mirror", edge("e", 5)).unwrap();
    assert_eq!(head(&graph), last + 1, "a new occurrence takes a sequence no reader has seen");
    assert!(graph.validate().unwrap().is_sound());
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
