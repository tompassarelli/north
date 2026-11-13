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

type RunLog = Box<dyn Fn(&str) -> Option<String>>;
type RunPage = Box<dyn Fn(u32) -> Result<Option<Vec<MainRun>>, String>>;

pub(crate) struct MainRuns {
    pub page: RunPage,
    pub log: RunLog,
    pub alert: Box<dyn Fn(&str)>,
}

impl MainRuns {
    pub(crate) fn main() -> Self {
        Self { page: Box::new(page), log: Box::new(log), alert: Box::new(super::alert::alert_proxy) }
    }

    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: FIXTURE_ANCESTOR.to_owned(), id: "0".to_owned(), red: false }]))), log: Box::new(|_| None), alert: Box::new(|_| {}) }
    }
}

#[derive(Deserialize)]
struct Row {
    id: u64,
    conclusion: String,
    head_sha: String,
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

fn log(run: &str) -> Option<String> {
    gh(&["run", "view", run, "-R", REPOSITORY, "--log-failed"]).ok()
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
