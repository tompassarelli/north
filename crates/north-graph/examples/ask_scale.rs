use std::collections::BTreeSet;
use std::time::Instant;

use north_graph::{Action, Graph, Term, Triple, parse_query};
use serde_json::json;

const NODES: i64 = 190_000;
const RETRACTED_NODES: i64 = 50_000;
const BATCH: usize = 10_000;
const RUNS: usize = 20;
const BOUND_MS: f64 = 50.0;

fn neighbours(node: i64, retracted: bool) -> Vec<i64> {
    let mut found = vec![(node * 7 + 1) % NODES, (node * 13 + 5) % NODES, (node * 31 + 9) % NODES, (node * 17 + 3) % NODES];
    if !retracted {
        found.push((node + 1) % NODES);
    }
    found
}

fn name(node: i64) -> Term {
    Term::string(format!("n{node}"))
}

fn edge(from: i64, to: i64) -> Triple {
    Triple::new(name(from), Term::keyword("edge"), name(to))
}

fn generate(graph: &mut Graph) -> usize {
    let mut actions = Vec::with_capacity(BATCH);
    let mut occurrences = 0;
    let mut flush = |graph: &mut Graph, actions: &mut Vec<Action>| {
        occurrences += actions.len();
        graph.apply("generator", actions).unwrap();
        actions.clear();
    };
    for node in 0..NODES {
        for target in neighbours(node, false) {
            actions.push(Action::assert(edge(node, target)));
            if actions.len() == BATCH {
                flush(graph, &mut actions);
            }
        }
    }
    for node in 0..RETRACTED_NODES {
        actions.push(Action::retract(edge(node, (node + 1) % NODES)));
        if actions.len() == BATCH {
            flush(graph, &mut actions);
        }
    }
    flush(graph, &mut actions);
    occurrences
}

fn three_hops_from(start: i64) -> BTreeSet<i64> {
    let step = |frontier: BTreeSet<i64>| -> BTreeSet<i64> { frontier.into_iter().flat_map(|node| neighbours(node, node < RETRACTED_NODES)).collect() };
    step(step(step(BTreeSet::from([start]))))
}

fn hop(from: serde_json::Value, to: serde_json::Value) -> serde_json::Value {
    json!({"rel": "triple", "args": [from, ["keyword", "edge"], to]})
}

fn main() {
    let directory = tempfile::tempdir().unwrap();
    let mut graph = Graph::open(&directory.path().join("graph.db")).unwrap();
    let started = Instant::now();
    let occurrences = generate(&mut graph);
    println!("generated {occurrences} occurrences in {:.1} s", started.elapsed().as_secs_f64());
    assert!(occurrences >= 1_000_000);

    let start = 12_345;
    let query = parse_query(
        &json!({"find": "hop3", "rules": [{
            "head": {"rel": "hop3", "args": [{"var": "d"}]},
            "body": [hop(json!(format!("n{start}")), json!({"var": "b"})), hop(json!({"var": "b"}), json!({"var": "c"})), hop(json!({"var": "c"}), json!({"var": "d"}))]
        }]})
        .to_string(),
    )
    .unwrap();

    let expected: BTreeSet<Term> = three_hops_from(start).into_iter().map(name).collect();
    let mut timings = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let answer = graph.ask(&query).unwrap();
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
        let found: BTreeSet<Term> = answer.rows.into_iter().map(|mut row| row.remove(0)).collect();
        assert_eq!(found, expected);
    }
    timings.sort_by(f64::total_cmp);
    let median = (timings[RUNS / 2 - 1] + timings[RUNS / 2]) / 2.0;
    println!("3-hop ask over {occurrences} occurrences: {} rows, median {median:.3} ms of {RUNS} (min {:.3}, max {:.3})", expected.len(), timings[0], timings[RUNS - 1]);
    assert!(median < BOUND_MS, "median {median:.3} ms is not under {BOUND_MS} ms");

    let started = Instant::now();
    let report = graph.validate().unwrap();
    println!("validate: {} terms, {} occurrences, {} live triples in {:.1} s", report.terms, report.occurrences, report.live_triples, started.elapsed().as_secs_f64());
    assert!(report.is_sound(), "{:?}", report.problems);
}
