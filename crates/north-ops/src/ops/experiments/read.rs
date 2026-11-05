//! Readings of north flow's outcome metrics and of the incidents ledger: the outcome loop's cohort
//! over one window for the runs in an entry's scope, or one incident class's rows per 7 days.
use super::registry::{Metric, Scope, Window};
use super::verdict::Reading;
use crate::ledger::outcome::{Outcome, cohorts};

const WEEK_DAYS: f64 = 7.0;

/// One incident as a reading needs it: its class and when it was detected, in Unix seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Detected {
    pub class: String,
    pub at: f64,
}

/// What the ledger recorded: finished worker runs and detected incidents.
#[derive(Clone, Copy, Default)]
pub struct Recorded<'a> {
    pub outcomes: &'a [Outcome],
    pub incidents: &'a [Detected],
}

fn incident_rows(incidents: &[Detected], class: &str, window: Window) -> Reading {
    let (start, end) = (window.start.unix() as f64, window.end.unix() as f64);
    let n = incidents.iter().filter(|incident| incident.class == class && incident.at >= start && incident.at < end).count();
    let days = (window.end.0 - window.start.0) as f64;
    Reading { value: Some(n as f64 * WEEK_DAYS / days), n }
}

/// One metric over one window for the runs in `scope`, judged as of `now`.
pub fn measure(recorded: Recorded, metric: &Metric, scope: &Scope, window: Window, now: i64) -> Option<Reading> {
    if let Metric::IncidentRows(class) = metric {
        return Some(incident_rows(recorded.incidents, class, window));
    }
    let scoped: Vec<Outcome> = recorded.outcomes.iter().filter(|outcome| scope.tier.as_ref().is_none_or(|tier| &outcome.tier == tier) && scope.arm.as_ref().is_none_or(|arm| &outcome.arm == arm)).cloned().collect();
    let (cohort, _) = cohorts(&scoped, window.start.unix(), window.end.unix(), now);
    let count = |n: usize| Reading { value: Some(n as f64), n };
    match metric {
        Metric::Runs => Some(count(cohort.runs)),
        Metric::Landed => Some(count(cohort.landed)),
        Metric::ReworkRate => Some(Reading { value: cohort.rework_rate(), n: cohort.landed }),
        Metric::UsdPerSurviving => Some(Reading { value: cohort.usd_per_surviving(), n: cohort.surviving() }),
        Metric::FirstCommitP50Minutes => Some(Reading { value: cohort.first_commit_p50(), n: cohort.first_commit_minutes.len() }),
        Metric::IncidentRows(_) | Metric::Unread(_) => None,
    }
}
