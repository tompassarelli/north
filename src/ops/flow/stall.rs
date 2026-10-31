//! The stall verdict: whether main has stopped moving while lanes wait, or a landing job has run
//! past twice its own median, or its workflow's median when the job has no history.
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

/// A workflow's median whole-run minutes and, per job name, the median minutes of the runs where
/// that job executed its steps.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct WorkflowMedians {
    pub(crate) run: Option<f64>,
    pub(crate) jobs: BTreeMap<String, f64>,
}

pub(crate) type Medians = BTreeMap<String, WorkflowMedians>;

/// How many of a workflow's newest successful runs have their jobs read for per-job medians.
pub(crate) const JOB_HISTORY_RUNS: usize = 40;

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

fn successful_in_week<'a>(runs: impl IntoIterator<Item = &'a Value>, now: f64) -> Vec<(f64, f64, &'a Value)> {
    runs.into_iter()
        .filter(|item| landing_run(item) && item["conclusion"].as_str() == Some("success"))
        .filter_map(|item| {
            let updated = seconds(item["updatedAt"].as_str()?)?;
            let started = seconds(item["startedAt"].as_str()?)?;
            (now - 7.0 * 86_400.0..=now).contains(&updated).then_some((updated, (updated - started) / 60.0, item))
        })
        .collect()
}

pub(crate) fn workflow_median<'a>(runs: impl IntoIterator<Item = &'a Value>, now: f64) -> Option<f64> {
    let durations: Vec<f64> = successful_in_week(runs, now).into_iter().map(|(_, minutes, _)| minutes).collect();
    median(&durations)
}

/// The newest successful landing runs of the past week, the ones whose jobs are worth reading.
pub(crate) fn newest_run_ids<'a>(runs: impl IntoIterator<Item = &'a Value>, now: f64) -> Vec<u64> {
    let mut week = successful_in_week(runs, now);
    week.sort_by(|a, b| b.0.total_cmp(&a.0));
    week.into_iter().filter_map(|(_, _, item)| item["databaseId"].as_u64()).take(JOB_HISTORY_RUNS).collect()
}

/// Median minutes per job name over successful jobs of the given runs' REST job lists, counting
/// only runs where the job ran as many steps as in any run: a cache hit skips the steps that do
/// the work and finishes early.
pub(crate) fn job_medians(runs: &[Vec<Value>]) -> BTreeMap<String, f64> {
    let mut seen: BTreeMap<&str, Vec<(usize, f64)>> = BTreeMap::new();
    for job in runs.iter().flatten().filter(|job| job["conclusion"].as_str() == Some("success")) {
        let (Some(name), Some(started), Some(completed)) = (job["name"].as_str(), job["started_at"].as_str().and_then(seconds), job["completed_at"].as_str().and_then(seconds)) else { continue };
        let ran = job["steps"].as_array().map_or(0, |steps| steps.iter().filter(|step| step["conclusion"].as_str() != Some("skipped")).count());
        seen.entry(name).or_default().push((ran, (completed - started) / 60.0));
    }
    seen.into_iter()
        .filter_map(|(name, samples)| {
            let most = samples.iter().map(|(ran, _)| *ran).max()?;
            let minutes: Vec<f64> = samples.iter().filter(|(ran, _)| *ran == most).map(|(_, minutes)| *minutes).collect();
            Some((name.to_owned(), median(&minutes)?))
        })
        .collect()
}

/// Twice the job's own median minutes, or its workflow's when the job has no history.
pub(crate) fn limit_minutes(medians: &Medians, job: &Job) -> Option<f64> {
    let known = medians.get(&job.workflow)?;
    Some(2.0 * known.jobs.get(&job.name).copied().or(known.run)?)
}

fn overdue(facts: &Facts) -> Vec<Overdue> {
    facts
        .jobs
        .iter()
        .filter_map(|job| {
            let limit = limit_minutes(facts.medians, job)?;
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
