//! The jobs running on GitHub Actions: this account's busy-slot samples, and the landing workflows'
//! in-progress jobs with each workflow's median duration.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::guards::CACHE_SECONDS;
use super::host::{Host, Places, iso, read_json, seconds, write_atomic};
use super::stall::{Job, Medians, landing_run, workflow_median};

const MEDIAN_SECONDS: f64 = 6.0 * 3600.0;
const DAY: f64 = 86_400.0;

/// The REST jobs of a run, fetched once: `gh run view` leaves out each job's runner.
pub(crate) struct RunJobs<'a> {
    host: &'a dyn Host,
    slug: String,
    seen: RefCell<BTreeMap<u64, Vec<Value>>>,
}

impl<'a> RunJobs<'a> {
    pub(crate) fn new(host: &'a dyn Host, slug: &str) -> Self {
        Self { host, slug: slug.into(), seen: RefCell::default() }
    }

    pub(crate) fn of(&self, run_id: u64) -> Option<Vec<Value>> {
        if let Some(jobs) = self.seen.borrow().get(&run_id) {
            return Some(jobs.clone());
        }
        let path = format!("repos/{}/actions/runs/{run_id}/jobs?per_page=100", self.slug);
        let pages = self.host.json(&["gh", "api", "--paginate", "--slurp", &path])?;
        let jobs: Vec<Value> = pages.as_array()?.iter().flat_map(|page| page["jobs"].as_array().cloned().unwrap_or_default()).collect();
        self.seen.borrow_mut().insert(run_id, jobs.clone());
        Some(jobs)
    }
}

fn busy_now(host: &dyn Host, repos: &[String]) -> Option<u64> {
    let mut busy = 0;
    for slug in repos {
        let active = host.json(&["gh", "run", "list", "--repo", slug, "--status", "in_progress", "--limit", "1000", "--json", "databaseId"])?;
        let jobs = RunJobs::new(host, slug);
        for item in active.as_array()? {
            busy += jobs.of(item["databaseId"].as_u64()?)?.iter().filter(|job| job["status"] == "in_progress" && job["runner_name"].as_str().is_some_and(|name| !name.is_empty())).count() as u64;
        }
    }
    Some(busy)
}

fn recent_repos(host: &dyn Host, now: f64) -> Option<Vec<String>> {
    let since = iso(now - 7.0 * DAY);
    let listed = host.json(&["gh", "repo", "list", "--limit", "1000", "--json", "nameWithOwner,pushedAt"])?;
    Some(listed.as_array()?.iter().filter(|item| item["pushedAt"].as_str().unwrap_or_default() >= since.as_str()).filter_map(|item| Some(item["nameWithOwner"].as_str()?.to_owned())).collect())
}

fn saved_samples(saved: &Value) -> Vec<(f64, u64)> {
    saved["samples"].as_array().into_iter().flatten().filter_map(|pair| Some((pair[0].as_f64()?, pair[1].as_u64()?))).collect()
}

/// The last hour of busy-job counts across the account's recently pushed repositories, sampled at
/// most every five minutes.
pub(crate) fn account_busy(places: &Places, host: &dyn Host, now: f64) -> Vec<(f64, u64)> {
    let path = places.flow().join("actions-busy.json");
    let saved = read_json(&path).unwrap_or_else(|| json!({}));
    let mut samples = saved_samples(&saved);
    if samples.last().is_some_and(|(at, _)| (0.0..CACHE_SECONDS).contains(&(now - at))) {
        return samples;
    }
    let fresh = saved["repos_epoch"].as_f64().is_some_and(|epoch| (0.0..MEDIAN_SECONDS).contains(&(now - epoch)));
    let known: Option<Vec<String>> = fresh.then(|| saved["repos"].as_array().map(|repos| repos.iter().filter_map(|repo| repo.as_str().map(str::to_owned)).collect())).flatten();
    let repos = match known.clone().or_else(|| recent_repos(host, now)) {
        Some(repos) => repos,
        None => return samples,
    };
    let Some(busy) = busy_now(host, &repos) else { return samples };
    samples.retain(|(at, _)| now - at <= 3600.0);
    samples.push((now, busy));
    let repos_epoch = if known.is_some() { saved["repos_epoch"].clone() } else { json!(now) };
    let document = json!({ "samples": samples, "repos": repos, "repos_epoch": repos_epoch });
    let _ = write_atomic(&path, &format!("{document}\n"));
    samples
}

fn workflows_of(host: &dyn Host, checkout: &Path) -> Option<Vec<String>> {
    let config = host.run(&["git", "config", "--blob", "origin/main:.safe-push", "--get-all", "ci.budget"], Some(checkout), None);
    let mut workflows: Vec<String> = config.unwrap_or_default().lines().filter_map(|line| line.split_whitespace().next().map(str::to_owned)).collect();
    let files = host.run(&["git", "ls-tree", "--name-only", "origin/main", ".github/workflows/autoland.yml"], Some(checkout), None)?;
    if !files.trim().is_empty() {
        workflows.push("autoland.yml".into());
    }
    let mut unique = Vec::new();
    for workflow in workflows {
        if !unique.contains(&workflow) {
            unique.push(workflow);
        }
    }
    Some(unique)
}

const DAILY_SAMPLE: &str = "100";

fn history_median(host: &dyn Host, slug: &str, workflow: &str, now: f64) -> Option<Option<f64>> {
    let mut history: BTreeMap<u64, Value> = BTreeMap::new();
    for day in 0..7 {
        let bounds = format!("{}..{}", iso(now - f64::from(day + 1) * DAY), iso(now - f64::from(day) * DAY));
        let mut command = vec!["gh", "run", "list", "--repo", slug, "--workflow", workflow, "--status", "success", "--created", &bounds, "--limit", DAILY_SAMPLE, "--json", "databaseId,workflowName,displayTitle,conclusion,startedAt,updatedAt"];
        if workflow == "autoland.yml" {
            command.extend(["--event", "workflow_dispatch"]);
        }
        let samples = host.json(&command)?;
        history.extend(samples.as_array()?.iter().filter_map(|item| Some((item["databaseId"].as_u64()?, item.clone()))));
    }
    Some(workflow_median(history.values(), now))
}

fn running_jobs(host: &dyn Host, slug: &str, workflow: &str, runs: &RunJobs) -> Option<Vec<Job>> {
    let active = host.json(&["gh", "run", "list", "--repo", slug, "--workflow", workflow, "--status", "in_progress", "--limit", "1000", "--json", "databaseId,workflowName,displayTitle"])?;
    let mut jobs = Vec::new();
    for item in active.as_array()?.iter().filter(|item| landing_run(item)) {
        let run_id = item["databaseId"].as_u64()?;
        let view = host.json(&["gh", "run", "view", &run_id.to_string(), "--repo", slug, "--json", "jobs"])?;
        let runners: BTreeMap<u64, String> = runs.of(run_id)?.iter().filter_map(|job| Some((job["id"].as_u64()?, job["runner_name"].as_str()?.to_owned()))).collect();
        for job in view["jobs"].as_array()?.iter().filter(|job| job["status"] == "in_progress") {
            let runner = runners.get(&job["databaseId"].as_u64()?).filter(|runner| !runner.is_empty()).cloned();
            jobs.push(Job { workflow: workflow.into(), run_id, name: job["name"].as_str()?.into(), started_at: seconds(job["startedAt"].as_str()?)?, runner: runner.unwrap_or_else(|| "waiting".into()) });
        }
    }
    Some(jobs)
}

fn saved_medians(path: &Path, now: f64) -> Medians {
    let Some(saved) = read_json(path).filter(|saved| saved["epoch"].as_f64().is_some_and(|epoch| (0.0..MEDIAN_SECONDS).contains(&(now - epoch)))) else { return Medians::new() };
    saved["medians"].as_object().into_iter().flatten().map(|(name, median)| (name.clone(), median.as_f64())).collect()
}

/// The landing workflows' jobs in progress and each workflow's median minutes over the past week,
/// the medians cached for six hours; `None` when GitHub cannot say.
pub(crate) fn landing_jobs(places: &Places, host: &dyn Host, checkout: &Path, slug: &str, now: f64) -> Option<(Vec<Job>, Medians)> {
    let workflows = workflows_of(host, checkout)?;
    let path = places.flow().join(format!("{}-medians.json", slug.replace('/', "--")));
    let saved = saved_medians(&path, now);
    let runs = RunJobs::new(host, slug);
    let mut jobs = Vec::new();
    let mut medians = Medians::new();
    for workflow in &workflows {
        let median = match saved.get(workflow) {
            Some(median) => *median,
            None => history_median(host, slug, workflow, now)?,
        };
        medians.insert(workflow.clone(), median);
        jobs.extend(running_jobs(host, slug, workflow, &runs)?);
    }
    if medians != saved {
        let _ = write_atomic(&path, &format!("{}\n", json!({ "epoch": now, "medians": medians })));
    }
    Some((jobs, medians))
}
