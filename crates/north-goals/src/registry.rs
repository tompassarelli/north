//! A project's goals as checks: `goals.toml` at its root names each goal drawn from the README or
//! AGENTS.md, the oracle command that measures it, the bound it must meet and its class.
use serde::Deserialize;

pub const PATH: &str = "goals.toml";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    Contract,
    Debt,
}

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Debt => "debt-allowed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bound {
    Max(f64),
    Min(f64),
}

impl Bound {
    pub fn holds(self, value: f64) -> bool {
        match self {
            Self::Max(limit) => value <= limit,
            Self::Min(limit) => value >= limit,
        }
    }

    pub fn limit(self) -> f64 {
        match self {
            Self::Max(limit) | Self::Min(limit) => limit,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Max(_) => "max",
            Self::Min(_) => "min",
        }
    }

    pub fn looser(self, ceiling: f64) -> bool {
        match self {
            Self::Max(limit) => ceiling > limit,
            Self::Min(limit) => ceiling < limit,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Debt {
    pub ceiling: f64,
    pub owner: String,
    pub revisit: String,
    pub pays: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    pub name: String,
    pub source: String,
    pub oracle: String,
    pub unit: String,
    pub bound: Bound,
    pub class: Class,
    pub debt: Option<Debt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    source: String,
    oracle: String,
    #[serde(default)]
    unit: String,
    max: Option<f64>,
    min: Option<f64>,
    class: String,
    ceiling: Option<f64>,
    owner: Option<String>,
    revisit: Option<String>,
    pays: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    goal: Vec<Raw>,
}

impl Raw {
    fn goal(self) -> Result<Goal, String> {
        let name = self.name.clone();
        let fail = |why: &str| format!("goal {name}: {why}");
        let bound = match (self.max, self.min) {
            (Some(limit), None) => Bound::Max(limit),
            (None, Some(limit)) => Bound::Min(limit),
            _ => return Err(fail("needs exactly one of max and min")),
        };
        if self.name.trim().is_empty() || self.source.trim().is_empty() || self.oracle.trim().is_empty() {
            return Err(fail("needs a name, the README or AGENTS.md line it comes from and an oracle command"));
        }
        let (class, debt) = match (self.class.as_str(), self.ceiling, self.owner, self.revisit) {
            ("contract", None, None, None) if self.pays.is_none() => (Class::Contract, None),
            ("contract", ..) => return Err(fail("a contract has no ceiling, owner, revisit trigger or paying paths; it is never red")),
            ("debt", Some(ceiling), Some(owner), Some(revisit)) if bound.looser(ceiling) && !owner.trim().is_empty() && !revisit.trim().is_empty() => (Class::Debt, Some(Debt { ceiling, owner, revisit, pays: self.pays.unwrap_or_default() })),
            ("debt", ..) => return Err(fail("debt-allowed needs a ceiling looser than its bound, an owner and a revisit trigger")),
            (other, ..) => return Err(fail(&format!("class is contract or debt, not {other}"))),
        };
        Ok(Goal { name: self.name, source: self.source, oracle: self.oracle, unit: self.unit, bound, class, debt })
    }
}

pub fn parse(text: &str) -> Result<Vec<Goal>, Vec<String>> {
    let value = north_base::platform::toml_json::parse(text).map_err(|why| vec![format!("{PATH}: {why}")])?;
    let file: File = serde_json::from_value(value).map_err(|why| vec![format!("{PATH}: {why}")])?;
    let (goals, problems): (Vec<_>, Vec<_>) = file.goal.into_iter().map(Raw::goal).partition(Result::is_ok);
    let mut problems: Vec<String> = problems.into_iter().filter_map(Result::err).collect();
    let goals: Vec<Goal> = goals.into_iter().filter_map(Result::ok).collect();
    let mut seen = std::collections::BTreeSet::new();
    problems.extend(goals.iter().filter(|goal| !seen.insert(goal.name.as_str())).map(|goal| format!("goal {}: named twice", goal.name)));
    if problems.is_empty() { Ok(goals) } else { Err(problems) }
}
