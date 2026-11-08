use std::collections::BTreeSet;

use north_graph::{Atom, Graph, Term, Triple};

use crate::cells;
use crate::rules::{ask, tell_set};

/// One asserted triple of a writer: its subject, its predicate and its value.
pub type Fact = (String, String, Term);

/// Every fact of the writer a rule file names, as `(subject, predicate, value)` rows.
pub fn facts_from(graph: &Graph, rule: &'static str) -> Result<Vec<Fact>, String> {
    Ok(ask(graph, rule)?
        .iter()
        .filter_map(|row| match row.as_slice() {
            [subject, Term::Atom(Atom::Keyword(predicate)), value] => Some((cells::text(subject)?.to_owned(), predicate.clone(), value.clone())),
            _ => None,
        })
        .collect())
}

/// Tells one tick as one transaction: retracts each prior fact that moved or went, asserts each new one, and moves the writer's tick to `at`.
pub fn tell_facts_to(graph: &mut Graph, writer: &str, prior: &[Fact], facts: &[Fact], at: i64) -> Result<(), String> {
    let triples = |facts: &[Fact]| facts.iter().map(|(subject, predicate, value)| Triple::new(Term::string(subject.clone()), Term::keyword(predicate.clone()), value.clone())).collect::<BTreeSet<_>>();
    tell_set(graph, writer, &triples(prior), &triples(facts), at)
}
