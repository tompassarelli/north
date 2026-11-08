use north_graph::{Action, Atom, Graph, Term, Triple};

use crate::cells;
use crate::rules::{ask, tick_actions, ticks_from};

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
    let triple = |(subject, predicate, value): &Fact| Triple::new(Term::string(subject.clone()), Term::keyword(predicate.clone()), value.clone());
    let mut actions: Vec<Action> = prior.iter().filter(|fact| !facts.contains(fact)).map(|fact| Action::retract(triple(fact))).collect();
    actions.extend(facts.iter().filter(|fact| !prior.contains(fact)).map(|fact| Action::assert(triple(fact))));
    actions.extend(tick_actions(writer, ticks_from(graph)?.get(writer).copied(), at)?);
    graph.apply(writer, &actions).map(drop).map_err(|why| why.to_string())
}
