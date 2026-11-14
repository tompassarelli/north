//! The completed runs of main's Buck2 workflow (north#1115), read by the screen on the host that settles the fleet: the newest
//! completed run on a head's ancestry decides whether that head is tripped (trip.rs, review_hold.rs `Screen`).

use std::collections::BTreeSet;

use north_base::foundation::check_log::log_text;
use north_base::platform::process;
use serde::Deserialize;

const REPOSITORY: &str = "tompassarelli/north";
const WORKFLOW: &str = "buck2.yml";
const PER_PAGE: &str = "100";
#[cfg(test)]
pub(crate) const FIXTURE_ANCESTOR: &str = "0000000000000000000000000000000000000000";
const GREEN: &str = "success";
const RED: [&str; 3] = ["failure", "timed_out", "startup_failure"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MainRun {
    pub head: String,
    pub id: String,
    pub red: bool,
}

const UNFINISHED: [&str; 2] = ["timed_out", "cancelled"];
const CUT_OFF: [&str; 2] = ["lost communication with the server", "exceeded the maximum execution time"];

/// What GitHub kept of a red run: its job record's word that a job was cut off before it finished, or its failed steps' log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kept {
    CutOff(String),
    Log(String),
}

type RunKept = Box<dyn Fn(&str) -> Result<Kept, String>>;
type RunPage = Box<dyn Fn(u32) -> Result<Option<Vec<MainRun>>, String>>;

pub(crate) struct MainRuns {
    pub page: RunPage,
    pub kept: RunKept,
    pub alert: Box<dyn Fn(&str)>,
}

impl MainRuns {
    pub(crate) fn main() -> Self {
        Self { page: Box::new(page), kept: Box::new(kept), alert: Box::new(super::alert::alert_proxy) }
    }

    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: FIXTURE_ANCESTOR.to_owned(), id: "0".to_owned(), red: false }]))), kept: Box::new(|_| Err("this fixture reads no run".to_owned())), alert: Box::new(|_| {}) }
    }
}

#[derive(Deserialize)]
struct Row {
    id: u64,
    conclusion: String,
    head_sha: String,
}

#[derive(Deserialize)]
struct Job {
    id: u64,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Annotation {
    annotation_level: String,
    message: String,
}

/// One page of main's completed Buck2 runs, newest first, the cancelled ones left out; `None` past the last run.
fn page(number: u32) -> Result<Option<Vec<MainRun>>, String> {
    let endpoint = format!("repos/{REPOSITORY}/actions/workflows/{WORKFLOW}/runs?branch=main&status=completed&per_page={PER_PAGE}&page={number}");
    let output = gh(&["api", endpoint.as_str(), "--jq", ".workflow_runs"])?;
    let rows: Vec<Row> = serde_json::from_str(&output).map_err(|error| format!("gh api runs of {WORKFLOW} page {number} is unreadable: {error}"))?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        rows.into_iter()
            .filter(|row| row.conclusion == GREEN || RED.contains(&row.conclusion.as_str()))
            .map(|row| MainRun { red: row.conclusion != GREEN, head: row.head_sha, id: row.id.to_string() })
            .collect(),
    ))
}

/// What GitHub kept of red run `run`: the failed steps' log once a job failed on its own, else why a job was cut off (it
/// timed out, was cancelled, or its failure annotation says its runner was lost or its time limit reached); a read that fails
/// is an error, so nothing is judged from it.
fn kept(run: &str) -> Result<Kept, String> {
    let endpoint = format!("repos/{REPOSITORY}/actions/runs/{run}/jobs");
    let jobs: Vec<Job> = serde_json::from_str(&gh(&["api", endpoint.as_str(), "--jq", ".jobs"])?).map_err(|error| format!("gh api jobs of run {run} is unreadable: {error}"))?;
    let mut cut_off = None;
    for job in jobs {
        let conclusion = job.conclusion.unwrap_or_default();
        let why = match conclusion.as_str() {
            "failure" => cut_off_annotation(job.id)?,
            unfinished if UNFINISHED.contains(&unfinished) => Some(format!("its job concluded {unfinished}")),
            _ => continue,
        };
        match why {
            Some(why) => cut_off = Some(why),
            None => return failed_log(run),
        }
    }
    cut_off.map_or_else(|| failed_log(run), |why| Ok(Kept::CutOff(why)))
}

fn cut_off_annotation(job: u64) -> Result<Option<String>, String> {
    let endpoint = format!("repos/{REPOSITORY}/check-runs/{job}/annotations");
    let annotations: Vec<Annotation> = serde_json::from_str(&gh(&["api", endpoint.as_str()])?).map_err(|error| format!("gh api annotations of job {job} is unreadable: {error}"))?;
    Ok(annotations.into_iter().find(|annotation| annotation.annotation_level == "failure" && CUT_OFF.iter().any(|cut| annotation.message.contains(cut))).map(|annotation| annotation.message))
}

fn failed_log(run: &str) -> Result<Kept, String> {
    gh(&["run", "view", run, "-R", REPOSITORY, "--log-failed"]).map(Kept::Log)
}

fn gh(arguments: &[&str]) -> Result<String, String> {
    let output = process::run("gh", None, arguments).map_err(|error| format!("gh {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("gh {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The failed crates and cases of a run's log: the nextest `FAIL [ TIME] (N/M) BINARY CASE` lines, each line read past its GitHub stamp and step.
pub(crate) fn failed_cases(log: &str) -> Vec<(String, String)> {
    let mut cases = BTreeSet::new();
    for line in log.lines() {
        let text = log_text(line);
        let text = match text.strip_prefix('[').and_then(|rest| rest.split_once("] ")) {
            Some((_, rest)) => rest.trim(),
            None => text,
        };
        let Some((took, rest)) = text.strip_prefix("FAIL [").and_then(|rest| rest.split_once(']')) else { continue };
        if !took.trim().ends_with('s') {
            continue;
        }
        let mut words = rest.split_whitespace().skip_while(|word| word.starts_with('(') && word.ends_with(')'));
        let (Some(binary), Some(case)) = (words.next(), words.next()) else { continue };
        let Some(crate_name) = binary.split("::").next() else { continue };
        cases.insert((crate_name.to_owned(), case.to_owned()));
    }
    cases.into_iter().collect()
}

/// Why a red run gives no verdict on its head, or None when it does: a job cut off before it finished, or a log that names no
/// failed case beside cases and targets that timed out, ran out of time rather than found a fault.
pub(crate) fn not_judged(kept: &Kept) -> Option<String> {
    match kept {
        Kept::CutOff(why) => Some(format!("its job was cut off before it finished: {why}")),
        Kept::Log(log) => {
            let timed_out = log.lines().map(log_text).any(|text| text.contains("TIMEOUT [") || text.contains("Timeout: "));
            (timed_out && failed_cases(log).is_empty()).then(|| "its cases and targets only timed out, none failed".to_owned())
        }
    }
}
