use std::collections::{BTreeMap, BTreeSet};

use crate::ops::detectors::{Detector, Finding};

pub struct Session {
    pub id: String,
    pub at: f64,
    pub steps: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub min_steps: usize,
    pub max_steps: usize,
    pub min_sessions: usize,
    pub window_s: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { min_steps: 3, max_steps: 8, min_sessions: 5, window_s: 7.0 * 24.0 * 3600.0 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern {
    pub steps: Vec<String>,
    pub sessions: BTreeSet<String>,
}

impl Pattern {
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn sequence(&self) -> String {
        self.steps.join(" > ")
    }

    pub fn subcommand(&self) -> String {
        let mut verbs: Vec<&str> = Vec::new();
        for step in &self.steps {
            let verb = step.rsplit(' ').next().unwrap_or(step);
            if !verbs.contains(&verb) {
                verbs.push(verb);
            }
        }
        format!("north {}", verbs.join("-"))
    }

    pub fn finding(&self, owner: Option<&str>, limits: &Limits) -> Finding {
        let sequence = self.sequence();
        let key = format!("sequence:{}", self.steps.join(">").replace(' ', "-"));
        Finding {
            detector: Detector::RecurringSequence,
            key: key.clone(),
            owner: owner.map(str::to_owned),
            title: format!("Make the recurring sequence `{sequence}` the subcommand `{}`", self.subcommand()),
            detail: format!(
                "{} sessions within {:.0} days ran the {}-step sequence `{sequence}`: {}.",
                self.count(),
                limits.window_s / 86_400.0,
                self.steps.len(),
                self.sessions.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            done_when: format!("`{}` runs the sequence in one call, and a week later `north tool-mine --dry-run` prints no `{key}` line.", self.subcommand()),
            instances: self.sessions.iter().cloned().collect(),
        }
    }
}

fn collapsed(steps: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for step in steps {
        if out.last() != Some(&step.as_str()) {
            out.push(step);
        }
    }
    out
}

fn contains(longer: &[String], shorter: &[String]) -> bool {
    longer.len() > shorter.len() && longer.windows(shorter.len()).any(|window| window == shorter)
}

pub fn recurring(sessions: &[Session], now: f64, limits: &Limits) -> Vec<Pattern> {
    let mut seen: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
    for session in sessions.iter().filter(|session| (now - limits.window_s..=now).contains(&session.at)) {
        let steps = collapsed(&session.steps);
        for length in limits.min_steps..=limits.max_steps.min(steps.len()) {
            for window in steps.windows(length) {
                seen.entry(window.iter().map(|step| (*step).to_owned()).collect()).or_default().insert(session.id.clone());
            }
        }
    }
    let frequent: Vec<Pattern> = seen.into_iter().filter(|(_, ids)| ids.len() >= limits.min_sessions).map(|(steps, sessions)| Pattern { steps, sessions }).collect();
    let mut maximal: Vec<Pattern> = frequent.iter().filter(|pattern| !frequent.iter().any(|other| other.count() >= pattern.count() && contains(&other.steps, &pattern.steps))).cloned().collect();
    maximal.sort_by(|a, b| b.count().cmp(&a.count()).then_with(|| b.steps.len().cmp(&a.steps.len())).then_with(|| a.steps.cmp(&b.steps)));
    maximal
}
