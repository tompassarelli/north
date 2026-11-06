use serde_json::json;

use crate::query::{Query, parse_query};
use crate::store::{Action, ActionKind, Graph, Terms};
use crate::term::{Atom, Float, Term, Triple};
use crate::{parse_arg, term_from_json, term_to_json};

fn proposition() -> Triple {
    Triple::new("Alice", Term::keyword("contactable_at"), "alice@example.com")
}

fn query(value: serde_json::Value) -> Query {
    parse_query(&value.to_string()).unwrap()
}

fn rows(graph: &Graph, find: serde_json::Value) -> Vec<Vec<Term>> {
    graph.ask(&query(find)).unwrap().rows
}

fn id_of(graph: &Graph, term: &Term) -> Option<i64> {
    Terms::new(graph.connection()).stored_id_of(term).unwrap()
}

#[test]
fn atom_identity_is_kind_plus_canonical_payload() {
    let mut graph = Graph::open_in_memory().unwrap();
    let atoms = [Term::int(1), Term::float(1.0), Term::string("1"), Term::keyword("1"), Term::boolean(true), Term::instant(1, 0).unwrap()];
    let facts: Vec<Action> = atoms.iter().map(|atom| Action::assert(Triple::new(atom.clone(), atom.clone(), atom.clone()))).collect();
    graph.apply("test", &facts).unwrap();
    let ids: Vec<i64> = atoms.iter().map(|atom| id_of(&graph, atom).unwrap()).collect();
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), atoms.len(), "equal payloads of different kinds must not share an identity");
    assert_eq!(id_of(&graph, &Term::int(1)), Some(ids[0]));
}

#[test]
fn float_identity_uses_canonical_bits_so_nan_is_one_atom_and_signed_zero_is_two() {
    let quiet = Term::Atom(Atom::Float(Float::from_bits(0x7ff8_0000_0000_0000)));
    let other_nan = Term::Atom(Atom::Float(Float::from_bits(0x7ff8_0000_0000_0001)));
    assert_eq!(quiet, other_nan);
    assert_ne!(Term::float(0.0), Term::float(-0.0));
    let mut graph = Graph::open_in_memory().unwrap();
    for float in [&quiet, &other_nan, &Term::float(0.0), &Term::float(-0.0)] {
        graph.tell("test", Triple::new("x", Term::keyword("v"), float.clone())).unwrap();
    }
    let found = rows(&graph, json!({"find": "v", "rules": [{"head": {"rel": "v", "args": [{"var": "v"}]}, "body": [{"rel": "triple", "args": ["x", ["keyword", "v"], {"var": "v"}]}]}]}));
    assert_eq!(found.len(), 3, "NaN twice collapses to one; +0.0 and -0.0 stay apart");
}

#[test]
fn a_triple_nests_in_every_position_and_nesting_asserts_nothing() {
    let inner = Term::from(proposition());
    let nested_t1 = Triple::new(inner.clone(), Term::keyword("a"), "t1");
    let nested_t2 = Triple::new("t2", inner.clone(), true);
    let nested_t3 = Triple::new(Term::keyword("t3"), 42, inner.clone());
    let mut graph = Graph::open_in_memory().unwrap();
    let all = Triple::new(nested_t1.clone(), nested_t2, nested_t3);
    graph.tell("test", all.clone()).unwrap();
    assert_eq!(graph.show(&Term::from(nested_t1)).unwrap().len(), 1);
    assert!(id_of(&graph, &inner).is_some(), "the inner triple is interned as a term");
    assert!(graph.show(&Term::string("Alice")).unwrap().is_empty(), "interning a nested triple does not assert it");
    let live = rows(&graph, json!({"find": "t", "rules": [{"head": {"rel": "t", "args": [{"var": "a"}, {"var": "b"}, {"var": "c"}]}, "body": [{"rel": "triple", "args": [{"var": "a"}, {"var": "b"}, {"var": "c"}]}]}]}));
    assert_eq!(live, vec![vec![all.t1, all.t2, all.t3]]);
}

#[test]
fn equal_propositions_are_distinct_occurrences_and_one_triple_row() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first = graph.tell("a", proposition()).unwrap();
    let second = graph.tell("b", proposition()).unwrap();
    assert_ne!(first.coordinate, second.coordinate);
    let triples = rows(&graph, json!({"find": "t", "rules": [{"head": {"rel": "t", "args": [{"var": "a"}]}, "body": [{"rel": "triple", "args": [{"var": "a"}, {"var": "b"}, {"var": "c"}]}]}]}));
    assert_eq!(triples.len(), 1);
    assert_eq!(graph.show(&Term::string("Alice")).unwrap()[0].occurrences, 2);
    let occurrences = rows(&graph, json!({"find": "o", "rules": [{"head": {"rel": "o", "args": [{"var": "c"}]}, "body": [{"rel": "occurrence", "args": [{"var": "c"}, ["keyword", "assert"], {"var": "p"}]}]}]}));
    assert_eq!(occurrences.len(), 2);
}

#[test]
fn a_retraction_withdraws_the_newest_live_equal_assertion_once() {
    let mut graph = Graph::open_in_memory().unwrap();
    let older = graph.tell("w", proposition()).unwrap();
    let newer = graph.tell("w", proposition()).unwrap();
    let retraction = graph.retract("w", proposition()).unwrap();
    assert!(retraction.state_changed);
    assert_eq!(retraction.withdrew.as_ref(), Some(&newer.coordinate), "the newest live equal assertion is the one withdrawn");
    let withdrawals = rows(&graph, json!({"find": "w", "rules": [{"head": {"rel": "w", "args": [{"var": "r"}, {"var": "a"}]}, "body": [{"rel": "withdrawal", "args": [{"var": "r"}, {"var": "a"}]}]}]}));
    assert_eq!(withdrawals, vec![vec![retraction.coordinate, newer.coordinate]]);
    assert_eq!(graph.show(&Term::string("Alice")).unwrap()[0].occurrences, 1, "the older equal assertion stays live");
    let second = graph.retract("w", proposition()).unwrap();
    assert_eq!(second.withdrew.as_ref(), Some(&older.coordinate));
    assert!(graph.show(&Term::string("Alice")).unwrap().is_empty());
}

#[test]
fn a_retraction_with_no_live_match_is_recorded_without_a_withdrawal() {
    let mut graph = Graph::open_in_memory().unwrap();
    let receipt = graph.retract("w", proposition()).unwrap();
    assert!(!receipt.state_changed);
    assert_eq!(receipt.withdrew, None);
    assert_eq!(receipt.kind, ActionKind::Retract);
    let recorded = rows(&graph, json!({"find": "o", "rules": [{"head": {"rel": "o", "args": [{"var": "c"}]}, "body": [{"rel": "occurrence", "args": [{"var": "c"}, ["keyword", "retract"], {"var": "p"}]}]}]}));
    assert_eq!(recorded, vec![vec![receipt.coordinate]]);
    let withdrawals = rows(&graph, json!({"find": "w", "rules": [{"head": {"rel": "w", "args": [{"var": "r"}]}, "body": [{"rel": "withdrawal", "args": [{"var": "r"}, {"var": "a"}]}]}]}));
    assert!(withdrawals.is_empty());
}

#[test]
fn coordinates_are_ordinary_nested_triples_that_order_by_transaction_then_ordinal() {
    let mut graph = Graph::open_in_memory().unwrap();
    let batch = graph.apply("w", &[Action::assert(proposition()), Action::assert(proposition())]).unwrap();
    let next = graph.tell("w", proposition()).unwrap();
    let transaction = |term: &Term| term.as_triple().unwrap().t1.clone();
    assert_eq!(transaction(&batch[0].coordinate), transaction(&batch[1].coordinate));
    assert_ne!(transaction(&batch[1].coordinate), transaction(&next.coordinate));
    assert_eq!(batch[1].coordinate, Term::triple(Term::triple("graph", Term::keyword("kernel/tx-sequence"), 1), Term::keyword("kernel/op-ordinal"), 1));
    assert_eq!(next.coordinate, Term::triple(Term::triple("graph", Term::keyword("kernel/tx-sequence"), 2), Term::keyword("kernel/op-ordinal"), 0));
    assert!(graph.validate().unwrap().is_sound());
}

#[test]
fn an_instant_rejects_a_nanosecond_field_of_a_second_or_more() {
    assert!(Term::instant(0, 1_000_000_000).is_err());
    assert!(Term::instant(1_785_560_000, 123_456_789).is_ok());
}

#[test]
fn terms_round_trip_through_their_text_and_json_forms() {
    let terms = [
        Term::string("a \"quoted\" (word)"),
        Term::int(-7),
        Term::float(1.5),
        Term::float(-0.0),
        Term::float(f64::NAN),
        Term::boolean(false),
        Term::keyword("kernel/op-ordinal"),
        Term::instant(-5, 7).unwrap(),
        Term::triple(Term::triple("a", Term::keyword("b"), 3), "c d", Term::float(2.0)),
    ];
    for term in terms {
        assert_eq!(parse_arg(&term.to_string()).unwrap(), term, "text form {term}");
        assert_eq!(term_from_json(&term_to_json(&term)).unwrap(), term, "json form {term}");
    }
    assert_eq!(parse_arg("north#669").unwrap(), Term::string("north#669"));
    assert!(parse_arg("(a b").is_err());
}

#[test]
fn a_user_term_shaped_like_a_future_coordinate_does_not_block_later_writes() {
    let mut graph = Graph::open_in_memory().unwrap();
    graph.tell("test", proposition()).unwrap();
    let squatter = Term::triple(Term::triple("graph", Term::keyword("kernel/tx-sequence"), 2), Term::keyword("kernel/op-ordinal"), 0);
    graph.tell("test", Triple::new(squatter, Term::keyword("note"), "x")).unwrap();
    let receipt = graph.tell("test", proposition()).unwrap();
    assert_eq!(receipt.coordinate, Term::triple(Term::triple("graph", Term::keyword("kernel/tx-sequence"), 3), Term::keyword("kernel/op-ordinal"), 0));
    assert!(graph.validate().unwrap().is_sound());
}

#[test]
fn terms_nested_beyond_the_depth_limit_are_refused_before_they_are_stored() {
    let deep = format!("{}x{}", "(".repeat(300), " y z)".repeat(300));
    assert!(parse_arg(&deep).is_err());
    let mut term = Term::int(0);
    for _ in 0..300 {
        term = Term::triple(term, Term::keyword("k"), 1);
    }
    let mut graph = Graph::open_in_memory().unwrap();
    assert!(graph.tell("test", term.as_triple().unwrap().clone()).is_err());
    assert_eq!(graph.validate().unwrap().occurrences, 0);
}
