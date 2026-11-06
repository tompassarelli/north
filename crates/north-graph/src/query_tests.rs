use serde_json::{Value, json};

use crate::error::{GraphError, QueryError};
use crate::query::{Budget, parse_query};
use crate::store::Graph;
use crate::term::{Term, Triple};

fn edge(from: &str, to: &str) -> Triple {
    Triple::new(from, Term::keyword("edge"), to)
}

fn chain(graph: &mut Graph, edges: &[(&str, &str)]) {
    let actions: Vec<_> = edges.iter().map(|(from, to)| crate::Action::assert(edge(from, to))).collect();
    graph.apply("test", &actions).unwrap();
}

fn answer(graph: &Graph, query: Value) -> Result<Vec<Vec<Term>>, GraphError> {
    graph.ask(&parse_query(&query.to_string()).unwrap()).map(|answer| answer.rows)
}

fn strings(rows: Vec<Vec<Term>>) -> Vec<String> {
    rows.into_iter().map(|row| row.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")).collect()
}

fn edge_pattern(from: Value, to: Value) -> Value {
    json!({"rel": "triple", "args": [from, ["keyword", "edge"], to]})
}

fn reaches() -> Value {
    json!([
        {"head": {"rel": "reaches", "args": [{"var": "x"}, {"var": "y"}]}, "body": [edge_pattern(json!({"var": "x"}), json!({"var": "y"}))]},
        {"head": {"rel": "reaches", "args": [{"var": "x"}, {"var": "z"}]}, "body": [edge_pattern(json!({"var": "x"}), json!({"var": "y"})), {"rel": "reaches", "args": [{"var": "y"}, {"var": "z"}]}]}
    ])
}

#[test]
fn recursion_reaches_a_fixpoint_over_a_cycle() {
    let mut graph = Graph::open_in_memory().unwrap();
    chain(&mut graph, &[("a", "b"), ("b", "c"), ("c", "a"), ("c", "d")]);
    let found = answer(&graph, json!({"find": "reaches", "rules": reaches()})).unwrap();
    assert_eq!(found.len(), 12, "a, b and c each reach a, b, c and d");
    let from_d = answer(&graph, json!({"find": "reaches", "rules": reaches(), "orderBy": [{"column": 0, "direction": "desc"}], "limit": 2})).unwrap();
    assert_eq!(strings(from_d), vec!["\"c\" \"a\"", "\"c\" \"b\""]);
}

#[test]
fn a_constant_anchors_a_three_hop_join() {
    let mut graph = Graph::open_in_memory().unwrap();
    chain(&mut graph, &[("a", "b"), ("b", "c"), ("c", "d"), ("c", "e"), ("x", "c")]);
    let query = json!({"find": "hop3", "rules": [{
        "head": {"rel": "hop3", "args": [{"var": "d"}]},
        "body": [edge_pattern(json!("a"), json!({"var": "b"})), edge_pattern(json!({"var": "b"}), json!({"var": "c"})), edge_pattern(json!({"var": "c"}), json!({"var": "d"}))]
    }]});
    assert_eq!(strings(answer(&graph, query).unwrap()), vec!["\"d\"", "\"e\""]);
}

#[test]
fn stratified_negation_filters_by_an_earlier_stratum() {
    let mut graph = Graph::open_in_memory().unwrap();
    chain(&mut graph, &[("a", "b"), ("b", "c"), ("d", "c")]);
    let query = json!({"find": "leaf", "strata": [
        [{"head": {"rel": "source", "args": [{"var": "x"}]}, "body": [edge_pattern(json!({"var": "x"}), json!({"var": "y"}))]}],
        [{"head": {"rel": "leaf", "args": [{"var": "y"}]}, "body": [edge_pattern(json!({"var": "x"}), json!({"var": "y"})), {"rel": "source", "args": [{"var": "y"}], "neg": true}]}]
    ]});
    assert_eq!(strings(answer(&graph, query).unwrap()), vec!["\"c\""]);
}

#[test]
fn a_withdrawn_fact_leaves_the_triple_relation_but_stays_in_history() {
    let mut graph = Graph::open_in_memory().unwrap();
    chain(&mut graph, &[("a", "b")]);
    graph.retract("test", edge("a", "b")).unwrap();
    let live = json!({"find": "t", "rules": [{"head": {"rel": "t", "args": [{"var": "y"}]}, "body": [edge_pattern(json!("a"), json!({"var": "y"}))]}]});
    assert!(answer(&graph, live).unwrap().is_empty());
    let history = json!({"find": "h", "rules": [{"head": {"rel": "h", "args": [{"var": "c"}]}, "body": [{"rel": "occurrence", "args": [{"var": "c"}, {"var": "action"}, {"var": "p"}]}]}]});
    assert_eq!(answer(&graph, history).unwrap().len(), 2);
}

#[test]
fn aggregates_group_a_completed_relation_and_having_filters_the_groups() {
    let mut graph = Graph::open_in_memory().unwrap();
    chain(&mut graph, &[("a", "b"), ("a", "c"), ("a", "d"), ("b", "c")]);
    let query = json!({
        "find": {"rel": "degree", "group": [0], "agg": [{"op": "count"}, {"op": "count-distinct", "arg": 1}], "having": [{"op": "gt", "agg": 0, "val": 1}]},
        "rules": [{"head": {"rel": "degree", "args": [{"var": "n"}, {"var": "m"}]}, "body": [edge_pattern(json!({"var": "n"}), json!({"var": "m"}))]}]
    });
    let found = answer(&graph, query).unwrap();
    assert_eq!(found, vec![vec![Term::string("a"), Term::int(3), Term::int(3)]]);
}

#[test]
fn arithmetic_and_filters_bind_and_prune_and_invalid_arithmetic_drops_the_row() {
    let mut graph = Graph::open_in_memory().unwrap();
    let scores = [("a", 4), ("b", 0), ("c", 10)].map(|(name, score)| crate::Action::assert(Triple::new(name, Term::keyword("score"), score)));
    graph.apply("test", &scores).unwrap();
    let query = json!({"find": "ratio", "orderBy": [{"column": 0, "direction": "asc"}], "rules": [{
        "head": {"rel": "ratio", "args": [{"var": "n"}, {"var": "r"}]},
        "body": [
            {"rel": "triple", "args": [{"var": "n"}, ["keyword", "score"], {"var": "s"}]},
            {"pred": "ge", "args": [{"var": "s"}, 0]},
            {"fn": "/", "args": [100, {"var": "s"}], "bind": "r"}
        ]
    }]});
    assert_eq!(answer(&graph, query).unwrap(), vec![vec![Term::string("a"), Term::int(25)], vec![Term::string("c"), Term::int(10)]]);
}

#[test]
fn malformed_queries_are_refused_before_anything_runs() {
    let graph = Graph::open_in_memory().unwrap();
    let head = |args: Value| json!({"rel": "r", "args": args});
    let refused = [
        ("unbound variable in the head", json!({"find": "r", "rules": [{"head": head(json!([{"var": "x"}])), "body": []}]})),
        ("negation reads an unbound variable", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "triple", "args": [{"var": "x"}, 1, 2], "neg": true}]}]})),
        ("unknown relation", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "nowhere", "args": []}]}]})),
        ("arity disagreement", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "withdrawal", "args": [1, 2, 3]}]}]})),
        ("negation within its own stratum", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "r", "args": [], "neg": true}]}]})),
        ("recursive arithmetic", json!({"find": "r", "rules": [{"head": head(json!([{"var": "n"}])), "body": [{"rel": "r", "args": [{"var": "m"}]}, {"fn": "+", "args": [{"var": "m"}, 1], "bind": "n"}]}]})),
        ("text relation", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "text-match", "args": ["a", "b", "c"]}]}]})),
        ("limit over the cap", json!({"find": "r", "limit": 100001, "rules": [{"head": head(json!([])), "body": []}]})),
    ];
    for (why, query) in refused {
        let outcome = answer(&graph, query);
        assert!(matches!(outcome, Err(GraphError::Query(QueryError::Invalid(_)))), "{why}: {outcome:?}");
    }
}

#[test]
fn a_query_that_exceeds_its_step_budget_stops_with_a_typed_error() {
    let mut graph = Graph::open_in_memory().unwrap();
    let edges: Vec<(String, String)> = (0..40).map(|i| (format!("n{i}"), format!("n{}", i + 1))).collect();
    let borrowed: Vec<(&str, &str)> = edges.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    chain(&mut graph, &borrowed);
    let query = parse_query(&json!({"find": "reaches", "rules": reaches()}).to_string()).unwrap();
    let outcome = graph.ask_within(&query, Budget::new(None).with_step_limit(500));
    assert!(matches!(outcome, Err(GraphError::Query(QueryError::StepBudgetExceeded { steps: 500 }))), "{outcome:?}");
    assert_eq!(graph.ask(&query).unwrap().rows.len(), 820, "the same query finishes inside the default budget");
}

#[test]
fn an_aggregate_over_an_empty_relation_yields_no_group_rather_than_an_error() {
    let graph = Graph::open_in_memory().unwrap();
    let query = json!({
        "find": {"rel": "all", "agg": [{"op": "max", "arg": 0}]},
        "rules": [{"head": {"rel": "all", "args": [{"var": "x"}]}, "body": [{"rel": "triple", "args": [{"var": "x"}, {"var": "y"}, {"var": "z"}]}]}]
    });
    assert!(answer(&graph, query).unwrap().is_empty());
}

#[test]
fn min_and_max_order_integers_and_floats_by_value() {
    let mut graph = Graph::open_in_memory().unwrap();
    let values = [Term::int(5), Term::float(1.5), Term::int(3)].map(|value| crate::Action::assert(Triple::new("x", Term::keyword("v"), value)));
    graph.apply("test", &values).unwrap();
    let query = json!({
        "find": {"rel": "v", "agg": [{"op": "min", "arg": 0}, {"op": "max", "arg": 0}]},
        "rules": [{"head": {"rel": "v", "args": [{"var": "v"}]}, "body": [{"rel": "triple", "args": ["x", ["keyword", "v"], {"var": "v"}]}]}]
    });
    assert_eq!(answer(&graph, query).unwrap(), vec![vec![Term::float(1.5), Term::int(5)]]);
}

#[test]
fn arithmetic_is_allowed_in_a_rule_that_does_not_read_its_own_relation_even_beside_a_recursive_sibling() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.apply("test", &[crate::Action::assert(Triple::new("a", Term::keyword("v"), 1))]).unwrap();
    let query = json!({"find": "h", "rules": [
        {"head": {"rel": "h", "args": [{"var": "x"}, {"var": "n"}]}, "body": [
            {"rel": "triple", "args": [{"var": "x"}, ["keyword", "v"], {"var": "m"}]},
            {"fn": "+", "args": [{"var": "m"}, 1], "bind": "n"}]},
        {"head": {"rel": "h", "args": [{"var": "x"}, {"var": "n"}]}, "body": [
            {"rel": "triple", "args": [{"var": "x"}, ["keyword", "e"], {"var": "y"}]},
            {"rel": "h", "args": [{"var": "y"}, {"var": "n"}]}]}
    ]});
    assert_eq!(answer(&graph, query).unwrap(), vec![vec![Term::string("a"), Term::int(2)]]);
}
