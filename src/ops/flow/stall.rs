//! The stall verdict: whether main has stopped moving while lanes wait, or a landing job has run
//! past twice its workflow's median.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::host::{median, seconds};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Job {
    pub(crate) workflow: String,
    pub(crate) run_id: u64,
    pub(crate) name: String,
    pub(crate) started_at: f64,
    pub(crate) runner: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Overdue {
    #[serde(flatten)]
    pub(crate) job: Job,
    pub(crate) age_minutes: f64,
    pub(crate) limit_minutes: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Verdict {
    pub(crate) stalled: bool,
    pub(crate) reason: String,
    pub(crate) main_age_minutes: f64,
    pub(crate) main_limit_minutes: f64,
    pub(crate) median_gap_minutes: Option<f64>,
    pub(crate) oldest_pending_minutes: Option<f64>,
    pub(crate) overdue_jobs: Vec<Overdue>,
    pub(crate) runner: Option<String>,
}

pub(crate) type Medians = BTreeMap<String, Option<f64>>;

pub(crate) struct Facts<'a> {
    pub(crate) now: f64,
    pub(crate) main_at: f64,
    pub(crate) landings: &'a [f64],
    pub(crate) pending: u64,
    pub(crate) jobs: &'a [Job],
    pub(crate) medians: &'a Medians,
    pub(crate) pending_since: Option<f64>,
}

/// A run record from `gh run list`, as a landing run or not; only the Autoland workflow's plan and
/// push runs are not landings.
pub(crate) fn landing_run(item: &Value) -> bool {
    if item["workflowName"].as_str() != Some("Autoland") {
        return true;
    }
    let title = item["displayTitle"].as_str().unwrap_or_default();
    title == "Autoland train"
        || title.starts_with("Autoland test ")
        || (title.starts_with("Autoland ") && !title.starts_with("Autoland plan") && !title.ends_with("(push)"))
}

pub(crate) fn workflow_median<'a>(runs: impl IntoIterator<Item = &'a Value>, now: f64) -> Option<f64> {
    let durations: Vec<f64> = runs
        .into_iter()
        .filter(|item| landing_run(item) && item["conclusion"].as_str() == Some("success"))
        .filter_map(|item| {
            let updated = seconds(item["updatedAt"].as_str()?)?;
            let started = seconds(item["startedAt"].as_str()?)?;
            (now - 7.0 * 86_400.0..=now).contains(&updated).then_some((updated - started) / 60.0)
        })
        .collect();
    median(&durations)
}

fn overdue(facts: &Facts) -> Vec<Overdue> {
    facts
        .jobs
        .iter()
        .filter_map(|job| {
            let limit = 2.0 * facts.medians.get(&job.workflow).copied().flatten()?;
            let minutes = (facts.now - job.started_at).max(0.0) / 60.0;
            (minutes > limit).then(|| Overdue { job: job.clone(), age_minutes: minutes, limit_minutes: limit })
        })
        .collect()
}

pub(crate) fn verdict(facts: &Facts) -> Verdict {
    let now = facts.now;
    let mut times: Vec<f64> = facts.landings.iter().copied().filter(|at| (now - 86_400.0..=now).contains(at)).collect();
    times.sort_by(f64::total_cmp);
    let gaps: Vec<f64> = times.windows(2).map(|pair| (pair[1] - pair[0]) / 60.0).collect();
    let gap = median(&gaps);
    let limit = gap.map_or(30.0, |gap| (3.0 * gap).max(30.0));
    let age = (now - facts.main_at).max(0.0) / 60.0;
    let waited = facts.pending_since.map(|since| (now - since).max(0.0) / 60.0);
    let overdue_jobs = overdue(facts);
    let mut reasons = Vec::new();
    if let Some(waited) = waited.filter(|waited| facts.pending > 0 && *waited > limit && age > limit) {
        reasons.push(format!(
            "oldest pending lane waiting {waited:.1}m and main idle {age:.1}m, both > {limit:.1}m, with {} pending",
            facts.pending
        ));
    }
    for late in &overdue_jobs {
        let job = &late.job;
        reasons.push(format!(
            "{} run {} {} running {:.1}m > {:.1}m on {}",
            job.workflow, job.run_id, job.name, late.age_minutes, late.limit_minutes, job.runner
        ));
    }
    let oldest = facts.jobs.iter().min_by(|a, b| a.started_at.total_cmp(&b.started_at));
    Verdict {
        stalled: !reasons.is_empty(),
        reason: if reasons.is_empty() { "healthy".into() } else { reasons.join("; ") },
        main_age_minutes: age,
        main_limit_minutes: limit,
        median_gap_minutes: gap,
        oldest_pending_minutes: waited,
        overdue_jobs,
        runner: oldest.map(|job| job.runner.clone()),
    }
}
