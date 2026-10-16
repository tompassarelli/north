use serde::Serialize;

use super::expect::{self, Expectation, Outcome, State};
use super::facts::{DAY, Facts};

const DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Verdict {
    Paying,
    Decayed,
    Unknown,
    Inactive,
}

impl Verdict {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Verdict::Paying => "PAYING",
            Verdict::Decayed => "DECAYED",
            Verdict::Unknown => "UNKNOWN",
            Verdict::Inactive => "INACTIVE",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Dividend {
    pub id: String,
    pub issues: Vec<String>,
    pub verdict: Verdict,
    pub daily: Vec<Option<f64>>,
    pub today: Outcome,
}

impl Dividend {
    pub(crate) fn line(&self) -> String {
        let daily = self.daily.iter().map(|value| value.map_or("-".to_owned(), |value| format!("{value:.2}"))).collect::<Vec<_>>().join(" ");
        format!(
            "{} {}: {} = {} (expected {} {} over {}); last 7 days oldest first: {daily}",
            self.id,
            self.verdict.label(),
            self.today.metric,
            self.today.value.map_or("unknown".to_owned(), |value| format!("{value:.2}")),
            self.today.op.symbol(),
            self.today.threshold,
            self.today.window,
        )
    }
}

pub(crate) fn evaluate(expectation: &Expectation, facts: &Facts) -> Dividend {
    let today = expect::evaluate(expectation, facts);
    let daily = (0..DAYS).rev().map(|back| expect::evaluate_at(expectation, facts, facts.now - back * DAY).value).collect();
    let verdict = match today.state {
        State::Holds => Verdict::Paying,
        State::Breached => Verdict::Decayed,
        State::Unknown => Verdict::Unknown,
        State::Inactive => Verdict::Inactive,
    };
    let issues = facts.bet_issues.iter().flatten().filter(|issue| issue.dividends.contains(&expectation.id)).map(|issue| issue.reference.clone()).collect();
    Dividend { id: expectation.id.clone(), issues, verdict, daily, today }
}

pub(crate) fn linked(expectations: &[Expectation], facts: &Facts) -> Vec<Dividend> {
    expectations
        .iter()
        .map(|expectation| evaluate(expectation, facts))
        .filter(|dividend| !dividend.issues.is_empty())
        .collect()
}

pub(crate) fn unknown_ids(expectations: &[Expectation], facts: &Facts) -> Vec<(String, String)> {
    facts
        .bet_issues
        .iter()
        .flatten()
        .flat_map(|issue| issue.dividends.iter().map(move |id| (issue.reference.clone(), id.clone())))
        .filter(|(_, id)| !expectations.iter().any(|expectation| expectation.id == *id))
        .collect()
}

pub(crate) fn comment(issue: &str, dividends: &[&Dividend], at: &str) -> String {
    let lines = dividends.iter().map(|dividend| format!("- {}", dividend.line())).collect::<Vec<_>>().join("\n");
    format!("**Dividend check {at}** (`north ops dividend`, trailing 7 days)\n\n{lines}\n\nIssue: {issue}")
}
