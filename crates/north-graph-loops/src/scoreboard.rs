use std::collections::BTreeSet;
use std::path::Path;

use north_graph::{Atom, Graph, Term, Triple};

use crate::cells;
use crate::rules::{ask, open_read_only, open_writable, tell_set, ticks_from};

pub const WRITER: &str = "scoreboard";
const RULE: &str = "harness/graph-rules/scoreboard.json";
const FIGURE: &str = "scoreboard/week-figure";
const OBSERVATIONS: &str = "scoreboard/observations";
const HIGHER_IS_BETTER: &str = "scoreboard/higher-is-better";

/// One number's headline figure for one week: `(number:NAME:YYYY-Www :scoreboard/week-figure F)` with the
/// observations it rests on and the direction that counts as better.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub number: String,
    pub week: String,
    pub figure: f64,
    pub observations: usize,
    pub higher_is_better: bool,
}

/// The readings the scoreboard last told and the instant it ticked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Told {
    pub readings: Vec<Reading>,
    pub at: Option<i64>,
}

impl Reading {
    fn subject(&self) -> Term {
        Term::string(format!("number:{}:{}", self.number, self.week))
    }

    fn facts(&self) -> Vec<Triple> {
        let on = |predicate: &str, object: Term| Triple::new(self.subject(), Term::keyword(predicate), object);
        vec![
            on(FIGURE, Term::float(self.figure)),
            on(OBSERVATIONS, Term::int(i64::try_from(self.observations).unwrap_or(i64::MAX))),
            on(HIGHER_IS_BETTER, Term::boolean(self.higher_is_better)),
        ]
    }

    fn of(row: &[Term]) -> Option<Self> {
        let (number, week) = cells::text(row.first()?)?.strip_prefix("number:")?.rsplit_once(':')?;
        let Term::Atom(Atom::Bool(higher_is_better)) = row.get(3)? else { return None };
        Some(Self {
            number: number.to_owned(),
            week: week.to_owned(),
            figure: cells::value(row.get(1)?)?.as_f64()?,
            observations: usize::try_from(cells::value(row.get(2)?)?.as_i64()?).ok()?,
            higher_is_better: *higher_is_better,
        })
    }
}

/// The readings the scoreboard told, through harness/graph-rules/scoreboard.json, and its newest tick.
pub fn read_from(graph: &Graph) -> Result<Told, String> {
    let mut readings: Vec<Reading> = ask(graph, RULE)?.iter().filter_map(|row| Reading::of(row)).collect();
    readings.sort_by(|left, right| (&left.number, &left.week).cmp(&(&right.number, &right.week)));
    Ok(Told { readings, at: ticks_from(graph)?.get(WRITER).copied() })
}

pub fn read(db: &Path) -> Result<Told, String> {
    read_from(&open_read_only(db)?)
}

/// Tells one recording as one transaction: the readings the graph held become `readings`, and the
/// scoreboard's tick moves to `at`.
pub fn tell_to(graph: &mut Graph, readings: &[Reading], at: i64) -> Result<(), String> {
    let prior = read_from(graph)?;
    let held: BTreeSet<Triple> = prior.readings.iter().flat_map(Reading::facts).collect();
    let new: BTreeSet<Triple> = readings.iter().flat_map(Reading::facts).collect();
    tell_set(graph, WRITER, &held, &new, at)
}

pub fn tell(db: &Path, readings: &[Reading], at: i64) -> Result<(), String> {
    tell_to(&mut open_writable(db)?, readings, at)
}
