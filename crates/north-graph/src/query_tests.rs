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
    graph.ask(&parse_query(&query.to_string())?).map(|answer| answer.rows)
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
        ("punctuation-only text needle", json!({"find": "r", "rules": [{"head": head(json!([])), "body": [{"rel": "text-match", "args": ["a", "b", "-;"]}]}]})),
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

fn restored_edge_history(graph: &mut Graph) {
    let filler = |n: i64| crate::Action::assert(Triple::new("x", Term::keyword("edge"), n));
    let actions = [
        crate::Action::assert(edge("a", "b")),
        filler(1),
        filler(2),
        filler(3),
        crate::Action::retract(edge("a", "b")),
        filler(4),
        filler(5),
        crate::Action::assert(edge("a", "b")),
    ];
    graph.apply("test", &actions).unwrap();
}

fn viewed(graph: &Graph, relation: Value, view: (&str, Value)) -> Result<Vec<Vec<Term>>, GraphError> {
    let head: Vec<Value> = relation["args"].as_array().unwrap().iter().filter(|arg| arg.get("var").is_some()).cloned().collect();
    let mut query = json!({"find": "seen", "rules": [{"head": {"rel": "seen", "args": head}, "body": [relation]}]});
    if !view.0.is_empty() {
        query[view.0] = view.1;
    }
    answer(graph, query)
}

fn all(relation: &str, arity: usize) -> Value {
    json!({"rel": relation, "args": (0..arity).map(|column| json!({"var": format!("v{column}")})).collect::<Vec<_>>()})
}

#[test]
fn as_of_cuts_show_a_retracted_then_restored_edge_at_each_sequence() {
    let mut graph = Graph::open_in_memory().unwrap();
    restored_edge_history(&mut graph);
    let from_a = edge_pattern(json!("a"), json!({"var": "y"}));
    let b_at = |cut: i64| viewed(&graph, from_a.clone(), ("asOf", json!(cut))).unwrap().len();
    assert_eq!([b_at(4), b_at(5), b_at(7), b_at(8)], [1, 0, 0, 1], "asserted at 1, retracted at 5, restored at 8");
    let assertions_at = |cut: i64| viewed(&graph, all("assertion", 4), ("asOf", json!(cut))).unwrap().len();
    assert_eq!([assertions_at(4), assertions_at(5), assertions_at(8)], [4, 3, 6]);
    let withdrawals_at = |cut: i64| viewed(&graph, all("withdrawal", 2), ("asOf", json!(cut))).unwrap().len();
    assert_eq!([withdrawals_at(4), withdrawals_at(5)], [0, 1]);
    assert_eq!(viewed(&graph, all("occurrence", 3), ("asOf", json!(6))).unwrap().len(), 6);
    assert_eq!(viewed(&graph, all("triple", 3), ("asOf", json!(8))).unwrap(), viewed(&graph, all("triple", 3), ("", Value::Null)).unwrap());
}

#[test]
fn as_of_since_window_keeps_what_happened_between_its_bounds() {
    let mut graph = Graph::open_in_memory().unwrap();
    restored_edge_history(&mut graph);
    let since = |relation: Value, from: i64, to: i64| strings(viewed(&graph, relation, ("since", json!([from, to]))).unwrap());
    assert_eq!(since(all("triple", 3), 6, 8), vec!["\"a\" :edge \"b\"", "\"x\" :edge 4", "\"x\" :edge 5"]);
    assert_eq!([since(all("triple", 3), 1, 4).len(), since(all("triple", 3), 1, 5).len()], [4, 3], "a to b is withdrawn at 5");
    assert_eq!(since(all("occurrence", 3), 5, 6).len(), 2);
    assert_eq!([since(all("withdrawal", 2), 5, 8).len(), since(all("withdrawal", 2), 6, 8).len()], [1, 0]);
}

#[test]
fn as_of_and_since_refuse_negative_future_and_reversed_bounds() {
    let mut graph = Graph::open_in_memory().unwrap();
    restored_edge_history(&mut graph);
    for (view, bound) in [("asOf", json!(-1)), ("asOf", json!(9)), ("since", json!([5, 4])), ("since", json!([-2, 3])), ("since", json!([1, 9]))] {
        let outcome = viewed(&graph, all("triple", 3), (view, bound.clone()));
        assert!(matches!(outcome, Err(GraphError::Query(QueryError::InvalidSnapshot(_)))), "{view} {bound}: {outcome:?}");
    }
    let both = json!({"find": "seen", "asOf": 3, "since": [1, 3], "rules": [{"head": {"rel": "seen", "args": []}, "body": []}]});
    assert!(matches!(answer(&graph, both), Err(GraphError::Query(QueryError::Invalid(_)))));
}

#[test]
fn an_instant_compares_with_an_instant_alike_on_a_base_clause_and_after_a_derived_one() {
    let mut graph = Graph::open_in_memory().unwrap();
    let at = |seconds| Term::instant(seconds, 0).unwrap();
    let actions = [crate::Action::assert(Triple::new("old", Term::keyword("at"), at(100))), crate::Action::assert(Triple::new("new", Term::keyword("at"), at(300)))];
    graph.apply("test", &actions).unwrap();
    let cutoff = json!(["instant", 200, 0]);
    let base = json!({"find": "before", "rules": [{"head": {"rel": "before", "args": [{"var": "x"}]}, "body": [
        {"rel": "triple", "args": [{"var": "x"}, ["keyword", "at"], {"var": "t"}]},
        {"pred": "lt", "args": [{"var": "t"}, cutoff]}]}]});
    let derived = json!({"find": "before", "strata": [
        [{"head": {"rel": "at", "args": [{"var": "x"}, {"var": "t"}]}, "body": [{"rel": "triple", "args": [{"var": "x"}, ["keyword", "at"], {"var": "t"}]}]}],
        [{"head": {"rel": "before", "args": [{"var": "x"}]}, "body": [
            {"rel": "at", "args": [{"var": "x"}, {"var": "t"}]},
            {"pred": "lt", "args": [{"var": "t"}, cutoff]}]}]]});
    assert_eq!(answer(&graph, base).unwrap(), vec![vec![Term::string("old")]]);
    assert_eq!(answer(&graph, derived).unwrap(), vec![vec![Term::string("old")]]);
}

fn prose(graph: &mut Graph, docs: &[(&str, &str)]) {
    let actions: Vec<_> = docs.iter().map(|(doc, text)| crate::Action::assert(Triple::new(*doc, Term::keyword("doc-text"), *text))).collect();
    graph.apply("grounding", &actions).unwrap();
}

fn text_hits(graph: &Graph, relation: &str, needle: Value) -> Result<Vec<String>, GraphError> {
    let rules = json!([{"head": {"rel": "hit", "args": [{"var": "d"}]}, "body": [{"rel": relation, "args": [{"var": "d"}, ["keyword", "doc-text"], needle]}]}]);
    let mut found = strings(answer(graph, json!({"find": "hit", "rules": rules}))?);
    found.sort();
    Ok(found)
}

fn refused(found: Result<Vec<String>, GraphError>) -> bool {
    matches!(found, Err(GraphError::Query(QueryError::Invalid(_))))
}

#[test]
fn text_match_is_an_unordered_conjunction_of_tokens_over_live_strings() {
    let mut graph = Graph::open_in_memory().unwrap();
    prose(&mut graph, &[("a", "the quick brown fox"), ("b", "Fox; brown!"), ("c", "quick red fox")]);
    graph.tell("grounding", Triple::new("a", Term::keyword("doc-title"), "brown fox")).unwrap();
    assert_eq!(text_hits(&graph, "text-match", json!("brown fox")).unwrap(), ["\"a\"", "\"b\""]);
    assert_eq!(text_hits(&graph, "text-match", json!("FOX, quick fox")).unwrap(), ["\"a\"", "\"c\""]);
    graph.retract("grounding", Triple::new("a", Term::keyword("doc-text"), "the quick brown fox")).unwrap();
    assert_eq!(text_hits(&graph, "text-match", json!("brown fox")).unwrap(), ["\"b\""], "a retracted string leaves the index");
    let any_attribute = json!([{"head": {"rel": "hit", "args": [{"var": "d"}, {"var": "a"}]}, "body": [{"rel": "text-match", "args": [{"var": "d"}, {"var": "a"}, "brown fox"]}]}]);
    assert_eq!(strings(answer(&graph, json!({"find": "hit", "rules": any_attribute, "orderBy": [{"column": 0, "direction": "asc"}]})).unwrap()), ["\"a\" :doc-title", "\"b\" :doc-text"]);
    graph.tell("test", Triple::new("q", Term::keyword("needle"), "red")).unwrap();
    let bound = json!([{"head": {"rel": "hit", "args": [{"var": "d"}]}, "body": [
        {"rel": "triple", "args": ["q", ["keyword", "needle"], {"var": "n"}]},
        {"rel": "text-match", "args": [{"var": "d"}, ["keyword", "doc-text"], {"var": "n"}]}
    ]}]);
    assert_eq!(strings(answer(&graph, json!({"find": "hit", "rules": bound})).unwrap()), ["\"c\""]);
    assert!(refused(text_hits(&graph, "text-match", json!(" -_ ;"))), "a punctuation-only needle");
    assert!(refused(text_hits(&graph, "text-match", json!(""))), "an empty needle");
    assert!(refused(text_hits(&graph, "text-match", json!(7))), "a needle that is not a String");
    assert!(refused(text_hits(&graph, "text-match", json!({"var": "n"}))), "an unbound needle");
    let negated = json!([{"head": {"rel": "hit", "args": [{"var": "d"}]}, "body": [
        {"rel": "triple", "args": [{"var": "d"}, ["keyword", "doc-text"], {"var": "t"}]},
        {"rel": "text-match", "args": [{"var": "d"}, ["keyword", "doc-text"], "fox"], "neg": true}
    ]}]);
    assert!(matches!(answer(&graph, json!({"find": "hit", "rules": negated})), Err(GraphError::Query(QueryError::Invalid(_)))));
    let rules = json!([{"head": {"rel": "hit", "args": [{"var": "d"}]}, "body": [{"rel": "text-match", "args": [{"var": "d"}, ["keyword", "doc-text"], "fox"]}]}]);
    assert!(matches!(answer(&graph, json!({"find": "hit", "rules": rules, "asOf": 1})), Err(GraphError::Query(QueryError::TextUnderView { .. }))));
}

#[test]
fn text_phrase_needs_the_tokens_in_order_across_punctuation() {
    let mut graph = Graph::open_in_memory().unwrap();
    prose(&mut graph, &[("a", "the quick brown fox"), ("b", "the brown quick fox"), ("c", "Quick-brown_FOX jumps"), ("d", "quick brown dog fox")]);
    assert_eq!(text_hits(&graph, "text-phrase", json!("quick brown fox")).unwrap(), ["\"a\"", "\"c\""]);
    assert_eq!(text_hits(&graph, "text-phrase", json!("brown quick")).unwrap(), ["\"b\""]);
}

#[test]
fn text_substring_is_case_folded_containment_that_keeps_punctuation() {
    let mut graph = Graph::open_in_memory().unwrap();
    prose(&mut graph, &[("a", "quick-brown_fox"), ("b", "quick brown fox"), ("c", "brown-own")]);
    assert_eq!(text_hits(&graph, "text-substring", json!("OWN_FO")).unwrap(), ["\"a\""]);
    assert_eq!(text_hits(&graph, "text-substring", json!("k-b")).unwrap(), ["\"a\""]);
    assert_eq!(text_hits(&graph, "text-substring", json!("n_")).unwrap(), ["\"a\""], "a needle too short for trigrams takes the exact scan");
    assert_eq!(text_hits(&graph, "text-substring", json!("own")).unwrap(), ["\"a\"", "\"b\"", "\"c\""]);
}

#[test]
fn text_stem_unifies_inflected_forms_and_leaves_text_match_exact() {
    let mut graph = Graph::open_in_memory().unwrap();
    prose(&mut graph, &[("a", "she runs daily"), ("b", "running late"), ("c", "a run home"), ("d", "rerun later")]);
    assert_eq!(text_hits(&graph, "text-stem", json!("run")).unwrap(), ["\"a\"", "\"b\"", "\"c\""]);
    assert_eq!(text_hits(&graph, "text-stem", json!("Running")).unwrap(), ["\"a\"", "\"b\"", "\"c\""]);
    assert_eq!(text_hits(&graph, "text-match", json!("run")).unwrap(), ["\"c\""]);
}

#[test]
fn text_search_scores_exact_over_stem_over_substring() {
    let mut graph = Graph::open_in_memory().unwrap();
    prose(&mut graph, &[("stem", "running late"), ("exact", "a run home"), ("substring", "rerun later"), ("none", "walk")]);
    let rules = json!([{"head": {"rel": "hit", "args": [{"var": "d"}, {"var": "s"}]}, "body": [{"rel": "text-search", "args": [{"var": "d"}, ["keyword", "doc-text"], "run", {"var": "s"}]}]}]);
    let rows = answer(&graph, json!({"find": "hit", "rules": rules, "orderBy": [{"column": 1, "direction": "desc"}]})).unwrap();
    let order: Vec<String> = rows.iter().map(|row| row[0].to_string()).collect();
    assert_eq!(order, ["\"exact\"", "\"stem\"", "\"substring\""]);
    assert!(rows.iter().all(|row| matches!(row[1], Term::Atom(crate::term::Atom::Float(_)))), "{rows:?}");
}
