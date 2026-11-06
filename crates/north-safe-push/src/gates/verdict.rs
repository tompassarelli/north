use std::collections::HashSet;

use super::plan::Gate;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Passed,
    Failed { detail: Option<String> },
}

impl Outcome {
    pub fn failed() -> Self {
        Self::Failed { detail: None }
    }

    pub fn failed_with(detail: impl Into<String>) -> Self {
        Self::Failed { detail: Some(detail.into()) }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Refusal {
    failures: Vec<String>,
}

impl Refusal {
    pub fn count(&self) -> usize {
        self.failures.len()
    }

    pub fn reason(&self) -> String {
        format!("{} blocking check(s) failed: {}", self.failures.len(), self.failures.join("; "))
    }

    pub fn line(&self) -> String {
        format!("safe-push: ✗ {}; NOT pushed.", self.reason())
    }
}

pub fn verdict(results: &[(Gate, Outcome)]) -> Result<(), Refusal> {
    let mut named = HashSet::new();
    let failures: Vec<String> = results
        .iter()
        .filter(|(gate, outcome)| *outcome != Outcome::Passed && named.insert(*gate))
        .map(|(gate, outcome)| match outcome {
            Outcome::Failed { detail: Some(detail) } => format!("{} {detail}", gate.label()),
            _ => gate.label().to_owned(),
        })
        .collect();
    if failures.is_empty() { Ok(()) } else { Err(Refusal { failures }) }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub head: String,
    pub destination: Option<String>,
    pub range: String,
}

pub struct Scanned {
    pub at: Snapshot,
    pub results: Vec<(Gate, Outcome)>,
}

pub fn reuse<'a>(scanned: &'a Scanned, now: &Snapshot) -> Option<&'a [(Gate, Outcome)]> {
    (scanned.at == *now).then_some(scanned.results.as_slice())
}

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod tests;
