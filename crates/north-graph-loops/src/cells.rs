use std::collections::BTreeMap;

use north_graph::{Atom, Term, Triple};
use serde_json::Value;

/// Every cell's value by its name, `<measure> <family> <category>`.
pub type Cells = BTreeMap<String, Value>;

/// What a writer's newest tick left in the graph: its cells and the instant it ticked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Learned {
    pub cells: Cells,
    pub at: Option<i64>,
}

/// A value north tune learns per cell, with the rule file its decision reads it through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Measure {
    EtaMinutes,
    BudgetUsd,
}

pub const TICK_AT: &str = "loop/tick-at";

impl Measure {
    pub const ALL: [Self; 2] = [Self::EtaMinutes, Self::BudgetUsd];

    pub fn name(self) -> &'static str {
        match self {
            Self::EtaMinutes => "eta_minutes",
            Self::BudgetUsd => "budget_usd",
        }
    }

    pub fn predicate(self) -> &'static str {
        match self {
            Self::EtaMinutes => "cell/eta-minutes",
            Self::BudgetUsd => "cell/budget-usd",
        }
    }

    pub fn rule(self) -> &'static str {
        match self {
            Self::EtaMinutes => "harness/graph-rules/eta.json",
            Self::BudgetUsd => "harness/graph-rules/budget.json",
        }
    }

    fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|measure| measure.name() == name)
    }

    /// The cell name a subject `cell:<category>:<family>` holds this measure under.
    pub fn cell(self, subject: &str) -> Option<String> {
        let (category, family) = subject.strip_prefix("cell:")?.rsplit_once(':')?;
        Some(format!("{} {family} {category}", self.name()))
    }
}

pub fn subject(writer: &str) -> String {
    format!("loop:{writer}")
}

/// The triple a named cell is told as, or none for a name or value outside the vocabulary.
pub fn triple(cell: &str, value: &Value) -> Option<Triple> {
    let words: Vec<&str> = cell.split(' ').collect();
    let (measure, family, category) = match words[..] {
        [measure, family, category] => (Measure::of(measure)?, family, category),
        _ => return None,
    };
    Some(Triple::new(Term::string(format!("cell:{category}:{family}")), Term::keyword(measure.predicate()), term(value)?))
}

fn term(value: &Value) -> Option<Term> {
    match value {
        Value::String(text) => Some(Term::string(text.clone())),
        Value::Number(number) => number.as_i64().map(Term::int).or_else(|| number.as_f64().map(Term::float)),
        _ => None,
    }
}

pub fn value(term: &Term) -> Option<Value> {
    match term {
        Term::Atom(Atom::String(text)) => Some(Value::from(text.clone())),
        Term::Atom(Atom::Int(number)) => Some(Value::from(*number)),
        Term::Atom(Atom::Float(number)) => Some(Value::from(number.value())),
        _ => None,
    }
}

pub fn text(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(Atom::String(text)) => Some(text),
        _ => None,
    }
}

pub fn seconds(term: &Term) -> Option<i64> {
    match term {
        Term::Atom(Atom::Instant(instant)) => Some(instant.seconds()),
        _ => None,
    }
}
