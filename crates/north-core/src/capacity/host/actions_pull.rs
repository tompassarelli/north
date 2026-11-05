use std::collections::BTreeMap;
use std::process::Command;

use rusqlite::Connection;
use serde_json::{Map, Value};

use super::fleet_pull::{insert_row, last_at, open_fleet_database, run_columns};
use crate::foundation::clock::Timestamp;
use crate::threads::incidents::Incident;

const OWNER: &str = "tompassarelli";
const REPOSITORIES: [&str; 3] = ["north", "makgora", "wisp"];
const FIRST_PULL_DAYS: i64 = 7;
const GENERIC_LABELS: [&str; 6] = ["self-hosted", "linux", "x64", "arm64", "macos", "windows"];

pub(super) type Gh<'a> = &'a dyn Fn(&str, &str, bool) -> Result<String, String>;
pub(super) type Record<'a> = &'a dyn Fn(&Incident) -> Result<bool, String>;

const INCIDENT_CLASS: &str = "main-red";
const INCIDENT_NOTICER: &str = "actions-pull";

fn gh_api(endpoint: &str, filter: &str, paginate: bool) -> Result<String, String> {
    let mut gh = Command::new("gh");
    gh.args(["api", endpoint]).args(paginate.then_some("--paginate")).args(["--jq", filter]);
    let output = gh.output().map_err(|error| format!("gh: {error}"))?;
    if !output.status.success() {
        return Err(format!("gh api {endpoint}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn seconds_between(from: &str, to: &str) -> Option<f64> {
    Some(Timestamp::parse_rfc3339(to)?.unix_seconds_f64() - Timestamp::parse_rfc3339(from)?.unix_seconds_f64())
}

fn runner_label(job: &Value) -> String {
    let labels: Vec<&str> = job["labels"].as_array().map(|labels| labels.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    labels.iter().find(|label| !GENERIC_LABELS.contains(label)).or(labels.first()).map_or_else(|| "unlabeled".to_owned(), |label| label.to_lowercase())
}

fn job_row(repo: &str, run: &str, job: &Value) -> Option<Map<String, Value>> {
    let (created, started, completed) = (job["created_at"].as_str()?, job["started_at"].as_str()?, job["completed_at"].as_str()?);
    let conclusion = job["conclusion"].as_str().unwrap_or("unknown");
    let mut row = Map::new();
    row.insert("source_id".into(), format!("actions:{repo}:{run}:{}", job["id"]).into());
    row.insert("at".into(), completed.into());
    row.insert("family".into(), "actions".into());
    row.insert("repo".into(), repo.into());
    row.insert("package".into(), job["name"].clone());
    row.insert("class".into(), "ci".into());
    row.insert("signature_exact".into(), 0.into());
    row.insert("owner".into(), job["workflow_name"].clone());
    row.insert("shape".into(), conclusion.into());
    row.insert("host".into(), format!("actions:{}", runner_label(job)).into());
    row.insert("queuedSeconds".into(), seconds_between(created, started)?.into());
    row.insert("wallSeconds".into(), seconds_between(started, completed)?.into());
    row.insert("exitCode".into(), i64::from(conclusion != "success").into());
    Some(row)
}

fn since_date(db: &Connection, repo: &str, now: Timestamp) -> Result<String, String> {
    let last = last_at(db, "host LIKE 'actions:%' AND repo = ?", repo).map_err(|error| error.to_string())?;
    let from = last.as_deref().and_then(Timestamp::parse_rfc3339).map_or_else(|| Timestamp::from_unix_seconds(now.unix_seconds() - FIRST_PULL_DAYS * 86_400), |at| Timestamp::from_unix_seconds(at.unix_seconds() - 86_400));
    Ok(from.iso_date())
}

pub(super) fn pull_repo(db: &mut Connection, repo: &str, now: Timestamp, gh: Gh<'_>) -> Result<usize, String> {
    let since = since_date(db, repo, now)?;
    let runs = gh(&format!("repos/{OWNER}/{repo}/actions/runs?status=completed&created=%3E%3D{since}&per_page=100"), ".workflow_runs[].id", true)?;
    let columns = run_columns(db);
    let mut added = 0;
    for run in runs.lines().map(str::trim).filter(|run| !run.is_empty()) {
        let known = db
            .query_row("SELECT 1 FROM capacity_runs WHERE source_id LIKE ? LIMIT 1", [format!("actions:{repo}:{run}:%")], |_| Ok(()))
            .is_ok();
        if known {
            continue;
        }
        let jobs = gh(&format!("repos/{OWNER}/{repo}/actions/runs/{run}/jobs?per_page=100"), ".jobs[]", true)?;
        let transaction = db.transaction().map_err(|error| error.to_string())?;
        for job in jobs.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
            if let Some(row) = job_row(repo, run, &job) {
                added += usize::from(insert_row(&transaction, &columns, &row).map_err(|error| error.to_string())?);
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
    }
    Ok(added)
}

pub(super) fn main_incident(repo: &str, run: &Value) -> Option<String> {
    let conclusion = run["conclusion"].as_str()?;
    if !matches!(conclusion, "failure" | "timed_out" | "startup_failure") {
        return None;
    }
    let text = |key: &str| run[key].as_str().unwrap_or("unknown");
    Some(format!("incident: {repo} main is red: {} concluded {conclusion} at {} {}", text("name"), text("updated_at"), text("html_url")))
}

pub(super) fn red_main_incident(repo: &str, run: &Value, now: Timestamp, record: Record<'_>) -> Result<Option<String>, String> {
    let Some(line) = main_incident(repo, run) else { return Ok(None) };
    let commit = run["head_sha"].as_str().filter(|commit| !commit.is_empty()).ok_or_else(|| format!("{repo}: the red main run names no commit"))?;
    let incident = Incident {
        key: format!("{INCIDENT_CLASS}:{repo}@{commit}"),
        class: INCIDENT_CLASS.to_owned(),
        noticed_by: INCIDENT_NOTICER.to_owned(),
        repo: repo.to_owned(),
        subject: commit.to_owned(),
        started_at: None,
        detected_at: now.iso(),
        detail: line.clone(),
    };
    Ok(record(&incident)?.then_some(line))
}

fn record_with_north(incident: &Incident) -> Result<bool, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(north)
        .args(["incident", "record", "--key", &incident.key, "--class", &incident.class, "--noticed-by", &incident.noticed_by])
        .args(["--repo", &incident.repo, "--subject", &incident.subject, "--detail", &incident.detail])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("north incident record: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).contains("already recorded"))
}

fn latest_main_run(repo: &str, gh: Gh<'_>) -> Result<Option<Value>, String> {
    let text = gh(&format!("repos/{OWNER}/{repo}/actions/runs?branch=main&event=push&status=completed&per_page=1"), ".workflow_runs[0] // empty", false)?;
    Ok(serde_json::from_str(text.trim()).ok())
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 { sorted[middle] } else { (sorted[middle - 1] + sorted[middle]) / 2.0 }
}

pub(super) fn queue_p50_by_label(db: &Connection) -> rusqlite::Result<Vec<(String, usize, f64)>> {
    let mut statement = db.prepare("SELECT host, queuedSeconds FROM capacity_runs WHERE host LIKE 'actions:%' AND queuedSeconds IS NOT NULL")?;
    let mut by_label: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for pair in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)))?.flatten() {
        by_label.entry(pair.0).or_default().push(pair.1);
    }
    Ok(by_label
        .into_iter()
        .map(|(label, mut queues)| {
            queues.sort_by(f64::total_cmp);
            let p50 = median(&queues);
            (label, queues.len(), p50)
        })
        .collect())
}

fn report(db: &Connection) -> rusqlite::Result<()> {
    let mut statement = db.prepare("SELECT host, repo, count(*) FROM capacity_runs WHERE host LIKE 'actions:%' GROUP BY host, repo ORDER BY host, repo")?;
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))?.flatten() {
        println!("{} {}: {} jobs", row.0, row.1, row.2);
    }
    for (label, jobs, p50) in queue_p50_by_label(db)? {
        println!("{label}: p50 queue {p50:.0}s over {jobs} jobs");
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north capacity actions-pull");
        return 2;
    }
    let mut db = match open_fleet_database(&crate::platform::host::name()) {
        Ok(db) => db,
        Err(error) => {
            eprintln!("north capacity actions-pull: {error}");
            return 1;
        }
    };
    let mut failed = false;
    for repo in REPOSITORIES {
        match pull_repo(&mut db, repo, Timestamp::now(), &gh_api) {
            Ok(added) => println!("{repo}: {added} new jobs"),
            Err(message) => {
                eprintln!("north capacity actions-pull: {repo}: {message}");
                failed = true;
            }
        }
    }
    if let Err(error) = report(&db) {
        eprintln!("north capacity actions-pull: {error}");
        failed = true;
    }
    let mut red = false;
    for repo in REPOSITORIES {
        match latest_main_run(repo, &gh_api) {
            Ok(run) => {
                if let Some(run) = run.filter(|run| main_incident(repo, run).is_some()) {
                    red = true;
                    match red_main_incident(repo, &run, Timestamp::now(), &record_with_north) {
                        Ok(line) => line.iter().for_each(|line| println!("{line}")),
                        Err(message) => {
                            eprintln!("north capacity actions-pull: {repo}: {message}");
                            failed = true;
                        }
                    }
                }
            }
            Err(message) => {
                eprintln!("north capacity actions-pull: {repo}: {message}");
                failed = true;
            }
        }
    }
    if failed { 1 } else if red { 3 } else { 0 }
}

#[cfg(test)]
#[path = "actions_pull_tests.rs"]
mod tests;
