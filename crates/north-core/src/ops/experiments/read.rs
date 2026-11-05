//! Readings of north flow's outcome metrics: the outcome loop's cohort over one window, for the runs
//! in an entry's scope.
use super::registry::{Metric, Scope, Window};
use super::verdict::Reading;
use crate::ledger::outcome::{Outcome, cohorts};

/// One metric over one window for the runs in `scope`, judged as of `now`.
pub fn measure(outcomes: &[Outcome], metric: &Metric, scope: &Scope, window: Window, now: i64) -> Option<Reading> {
    let scoped: Vec<Outcome> = outcomes.iter().filter(|outcome| scope.tier.as_ref().is_none_or(|tier| &outcome.tier == tier) && scope.arm.as_ref().is_none_or(|arm| &outcome.arm == arm)).cloned().collect();
    let (cohort, _) = cohorts(&scoped, window.start.unix(), window.end.unix(), now);
    let count = |n: usize| Reading { value: Some(n as f64), n };
    match metric {
        Metric::Runs => Some(count(cohort.runs)),
        Metric::Landed => Some(count(cohort.landed)),
        Metric::ReworkRate => Some(Reading { value: cohort.rework_rate(), n: cohort.landed }),
        Metric::UsdPerSurviving => Some(Reading { value: cohort.usd_per_surviving(), n: cohort.surviving() }),
        Metric::FirstCommitP50Minutes => Some(Reading { value: cohort.first_commit_p50(), n: cohort.first_commit_minutes.len() }),
        Metric::Unread(_) => None,
    }
}

