//! The skill switch: each learned value that drives a decision is scored walk-forward against the constant it would
//! replace, and drives its decision only while it beats that constant on the newest window. The backtest tells one
//! reading per decision, `(skill:DECISION :skill/drives :true|:false)` with the two scores, the runs behind them and
//! the constant's value on that window; every caller reads `verdict` and, when the value is set aside, decides on the
//! constant and says so in one line, while the value keeps learning in shadow.
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term};

use crate::cells;
use crate::facts::{Fact, facts_from, tell_facts_to};

pub const WRITER: &str = "backtest";
const RULE: &str = "harness/graph-rules/skill.json";
const SUBJECT: &str = "skill:";
const PREDICATE: &str = "skill/";
const PATIENCE: Duration = Duration::from_millis(400);
pub const ISSUE: &str = "north#1221";

/// A decision a learned value drives.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Decision {
    Eta,
    Budget,
    Cost,
    TierSuccess,
    ArmShares,
}

impl Decision {
    pub const ALL: [Self; 5] = [Self::Eta, Self::Budget, Self::Cost, Self::TierSuccess, Self::ArmShares];

    pub fn name(self) -> &'static str {
        match self {
            Self::Eta => "eta",
            Self::Budget => "budget",
            Self::Cost => "cost",
            Self::TierSuccess => "tier-success",
            Self::ArmShares => "arm-shares",
        }
    }

    /// The runs a reading needs before it can set a value aside or keep it.
    pub fn min_n(self) -> u64 {
        match self {
            Self::Eta | Self::Budget | Self::Cost => 300,
            Self::TierSuccess => 500,
            Self::ArmShares => 30,
        }
    }

    /// How far below the constant's score the learned value's must sit to count as beating it.
    pub fn margin(self) -> f64 {
        match self {
            Self::TierSuccess => 0.01,
            _ => 0.0,
        }
    }

    fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|decision| decision.name() == name)
    }
}

/// One decision's newest walk-forward reading: the learned value's mean loss and the constant's on the same runs
/// (lower is better), the runs behind them, and the constant's own value on the newest window.
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub decision: Decision,
    pub n: u64,
    pub learned: f64,
    pub constant: f64,
    pub value: Option<f64>,
}

impl Skill {
    pub fn judged(&self) -> bool {
        self.n >= self.decision.min_n()
    }

    pub fn beats(&self) -> bool {
        self.learned + self.decision.margin() < self.constant
    }

    fn facts(&self) -> Vec<Fact> {
        let subject = format!("{SUBJECT}{}", self.decision.name());
        let fact = |measure: &str, value: Term| (subject.clone(), format!("{PREDICATE}{measure}"), value);
        let mut told = vec![
            fact("n", Term::int(i64::try_from(self.n).unwrap_or(i64::MAX))),
            fact("learned", Term::float(self.learned)),
            fact("constant", Term::float(self.constant)),
            fact("drives", Term::keyword(if !self.judged() || self.beats() { "true" } else { "false" })),
        ];
        told.extend(self.value.map(|value| fact("value", Term::float(value))));
        told
    }
}

/// Whether a decision's learned value drives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Unjudged,
    Drives(Skill),
    SetAside(Skill),
}

impl Verdict {
    pub fn drives(&self) -> bool {
        !matches!(self, Self::SetAside(_))
    }

    /// The constant a set-aside decision uses, when the reading carries one.
    pub fn constant(&self) -> Option<f64> {
        match self {
            Self::SetAside(skill) => skill.value,
            _ => None,
        }
    }

    /// The one line a decision says when its learned value is set aside.
    pub fn line(&self) -> Option<String> {
        let Self::SetAside(skill) = self else { return None };
        Some(format!(
            "learned {} set aside for its constant: it loses walk-forward ({:.3} against {:.3}, n {}, {ISSUE})",
            skill.decision.name(),
            skill.learned,
            skill.constant,
            skill.n
        ))
    }
}

fn reading(decision: Decision, facts: impl Iterator<Item = (String, Term)>) -> Option<(Skill, bool)> {
    let mut skill = Skill { decision, n: 0, learned: f64::NAN, constant: f64::NAN, value: None };
    let mut drives = None;
    for (measure, value) in facts {
        let number = cells::value(&value).and_then(|value| value.as_f64());
        match (measure.as_str(), number, &value) {
            ("n", Some(n), _) => skill.n = n as u64,
            ("learned", Some(score), _) => skill.learned = score,
            ("constant", Some(score), _) => skill.constant = score,
            ("value", Some(constant), _) => skill.value = Some(constant),
            ("drives", _, Term::Atom(Atom::Keyword(word))) => drives = Some(word == "true"),
            _ => {}
        }
    }
    drives.map(|drives| (skill, drives))
}

pub fn verdict_from(graph: &Graph, decision: Decision) -> Result<Verdict, String> {
    let subject = format!("{SUBJECT}{}", decision.name());
    let live = graph.show(&Term::string(subject)).map_err(|why| why.to_string())?;
    let facts = live.into_iter().filter_map(|live| match live.triple.t2 {
        Term::Atom(Atom::Keyword(predicate)) => Some((predicate.strip_prefix(PREDICATE)?.to_owned(), live.triple.t3)),
        _ => None,
    });
    Ok(match reading(decision, facts) {
        None => Verdict::Unjudged,
        Some((skill, true)) => Verdict::Drives(skill),
        Some((skill, false)) => Verdict::SetAside(skill),
    })
}

/// The decision's verdict in the graph at `db`; a graph that cannot be read leaves the value driving, as before any reading.
pub fn verdict_in(db: &Path, decision: Decision) -> Verdict {
    Graph::open_read_only(db, PATIENCE).map_err(|why| why.to_string()).and_then(|graph| verdict_from(&graph, decision)).unwrap_or(Verdict::Unjudged)
}

pub fn verdict(decision: Decision) -> Verdict {
    verdict_in(&north_graph::default_path(), decision)
}

/// Every reading the backtest last told, by decision.
pub fn skills_from(graph: &Graph) -> Result<Vec<(Skill, bool)>, String> {
    let facts = facts_from(graph, RULE)?;
    Ok(Decision::ALL
        .into_iter()
        .filter_map(|decision| {
            let own = format!("{SUBJECT}{}", decision.name());
            reading(decision, facts.iter().filter(|(subject, ..)| *subject == own).filter_map(|(_, predicate, value)| Some((predicate.strip_prefix(PREDICATE)?.to_owned(), value.clone()))))
        })
        .collect())
}

/// Tells one backtest as one transaction: every decision's reading, replacing the last, and the writer's tick at `at`.
pub fn tell_skills_to(graph: &mut Graph, skills: &[Skill], at: i64) -> Result<(), String> {
    let prior = facts_from(graph, RULE)?;
    let told: Vec<Fact> = skills.iter().flat_map(Skill::facts).collect();
    tell_facts_to(graph, WRITER, &prior, &told, at)
}

pub fn tell_skills(db: &Path, skills: &[Skill], at: i64) -> Result<(), String> {
    tell_skills_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, skills, at)
}

pub fn decision(name: &str) -> Option<Decision> {
    Decision::of(name)
}
