pub mod attention;
pub mod read;
#[doc(hidden)]
pub mod replay_tests;

use std::collections::BTreeMap;

use crate::foundation::clock::Timestamp;
use crate::ledger::closed::tier_label;
use crate::threads::calibration::rework::{Judged, REWORK_WINDOW_S};
use attention::{Attention, Incidents};

pub const WEEK_S: i64 = 7 * 86_400;
const UNARMED: &str = "unarmed";
const ALL: &str = "all";

#[derive(Clone, Debug, PartialEq)]
pub struct Staffed {
    pub agent: String,
    pub tier: String,
    pub arm: String,
    pub ended: i64,
    pub landed_sha: Option<String>,
    pub landed_at: Option<i64>,
    pub first_commit: Option<i64>,
    pub usd: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landed {
    pub sha: String,
    pub landed_at: Option<i64>,
    pub first_commit_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    pub sha: String,
    pub at: i64,
    pub first_commit: Option<i64>,
    pub reworked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub agent: String,
    pub tier: String,
    pub arm: String,
    pub ended: i64,
    pub usd: f64,
    pub landing: Option<Landing>,
}

fn same_commit(left: &str, right: &str) -> bool {
    left.len().min(right.len()) >= 7 && (left.starts_with(right) || right.starts_with(left))
}

pub fn join(staffed: &[Staffed], landed: &[Landed], judged: &BTreeMap<String, Judged>) -> Vec<Outcome> {
    staffed
        .iter()
        .map(|run| {
            let landing = run.landed_sha.as_deref().filter(|sha| !sha.is_empty()).map(|sha| {
                let row = landed.iter().find(|row| same_commit(&row.sha, sha));
                let verdict = judged.get(&run.agent);
                Landing {
                    sha: sha.to_owned(),
                    at: verdict.map(|judged| judged.at).or(run.landed_at).or_else(|| row.and_then(|row| row.landed_at)).unwrap_or(run.ended),
                    first_commit: row.and_then(|row| row.first_commit_at).or(run.first_commit),
                    reworked: verdict.map(|judged| judged.cause.is_some()),
                }
            });
            Outcome { agent: run.agent.clone(), tier: tier_label(&run.tier), arm: if run.arm.is_empty() { UNARMED.to_owned() } else { run.arm.clone() }, ended: run.ended, usd: run.usd, landing }
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cohort {
    pub tier: String,
    pub arm: String,
    pub runs: usize,
    pub usd: f64,
    pub landed: usize,
    pub reworked: usize,
    pub unjudged: usize,
    pub maturing: usize,
    pub first_commit_minutes: Vec<f64>,
}

impl Cohort {
    pub fn surviving(&self) -> usize {
        self.landed - self.reworked
    }

    pub fn rework_rate(&self) -> Option<f64> {
        (self.landed > 0).then(|| self.reworked as f64 / self.landed as f64)
    }

    pub fn usd_per_surviving(&self) -> Option<f64> {
        (self.surviving() > 0).then(|| self.usd / self.surviving() as f64)
    }

    pub fn first_commit_p50(&self) -> Option<f64> {
        let mut sorted = self.first_commit_minutes.clone();
        sorted.sort_by(f64::total_cmp);
        let middle = sorted.len() / 2;
        match sorted.len() {
            0 => None,
            length if length % 2 == 1 => Some(sorted[middle]),
            _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
        }
    }

    fn add(&mut self, outcome: &Outcome, start: i64, end: i64, now: i64) {
        if start <= outcome.ended && outcome.ended < end {
            self.runs += 1;
            self.usd += outcome.usd;
        }
        let Some(landing) = outcome.landing.as_ref().filter(|landing| start <= landing.at && landing.at < end) else { return };
        self.landed += 1;
        match landing.reworked {
            Some(true) => self.reworked += 1,
            Some(false) if now - landing.at < REWORK_WINDOW_S => self.maturing += 1,
            Some(false) => {}
            None => self.unjudged += 1,
        }
        if let Some(first) = landing.first_commit {
            self.first_commit_minutes.push((landing.at - first).max(0) as f64 / 60.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Week {
    pub start: i64,
    pub end: i64,
    pub total: Cohort,
    pub cohorts: Vec<Cohort>,
    pub attention: Attention,
    pub incidents: Incidents,
}

pub fn cohorts(outcomes: &[Outcome], start: i64, end: i64, now: i64) -> (Cohort, Vec<Cohort>) {
    let mut by: BTreeMap<(String, String), Cohort> = BTreeMap::new();
    let mut total = Cohort { tier: ALL.into(), arm: ALL.into(), ..Cohort::default() };
    for outcome in outcomes {
        let cohort = by.entry((outcome.tier.clone(), outcome.arm.clone())).or_insert_with(|| Cohort { tier: outcome.tier.clone(), arm: outcome.arm.clone(), ..Cohort::default() });
        cohort.add(outcome, start, end, now);
        total.add(outcome, start, end, now);
    }
    (total, by.into_values().filter(|cohort| cohort.runs > 0 || cohort.landed > 0).collect())
}

fn percent(rate: Option<f64>) -> String {
    rate.map_or_else(|| "n/a".into(), |rate| format!("{:.0}%", rate * 100.0))
}

fn money(usd: Option<f64>) -> String {
    usd.map_or_else(|| "n/a".into(), |usd| format!("${usd:.2}"))
}

fn cohort_text(cohort: &Cohort, hours: f64) -> String {
    let per_hour = if hours > 0.0 { format!("{:.2}", cohort.landed as f64 / hours) } else { "n/a".into() };
    format!(
        "landed {} (reworked {}/{} = {}, {} not on any main, {} under 48 h), cost per surviving landing {} ({} over {} runs / {} surviving), first-commit-to-main p50 {} (n={}/{}), landed per hour of Tom's attention {per_hour} ({}/{hours:.1} h)",
        cohort.landed,
        cohort.reworked,
        cohort.landed,
        percent(cohort.rework_rate()),
        cohort.unjudged,
        cohort.maturing,
        money(cohort.usd_per_surviving()),
        money(Some(cohort.usd)),
        cohort.runs,
        cohort.surviving(),
        cohort.first_commit_p50().map_or_else(|| "n/a".into(), |minutes| format!("{minutes:.0}m")),
        cohort.first_commit_minutes.len(),
        cohort.landed,
        cohort.landed,
    )
}

pub fn lines(weeks: &[Week], sources: &str) -> Vec<String> {
    let mut lines = vec![format!("outcome sources: {sources}")];
    for week in weeks {
        let span = format!("[{}, {})", Timestamp::from_unix_seconds(week.start).iso(), Timestamp::from_unix_seconds(week.end).iso());
        let hours = week.attention.minutes / 60.0;
        lines.push(format!("outcome 7d {span}: {}", cohort_text(&week.total, hours)));
        lines.push(format!("outcome 7d {span}: {}", week.attention.line()));
        lines.push(format!("outcome 7d {span}: {}", week.incidents.line()));
        for cohort in &week.cohorts {
            lines.push(format!("outcome 7d {span} {} arm {}: {}", cohort.tier, cohort.arm, cohort_text(cohort, hours)));
        }
    }
    lines
}

pub fn weeks(outcomes: &[Outcome], tom: &attention::Messages, corrections: &[attention::Correction], bugs: &[attention::Bug], now: i64) -> Vec<Week> {
    [(now - WEEK_S, now), (now - 2 * WEEK_S, now - WEEK_S)]
        .into_iter()
        .map(|(start, end)| {
            let (total, cohorts) = cohorts(outcomes, start, end, now);
            let attention = tom.attention(corrections, start, end);
            let incidents = tom.incidents(corrections, bugs, start, end);
            Week { start, end, total, cohorts, attention, incidents }
        })
        .collect()
}
