//! The completed runs of main's Buck2 workflow (north#1115), read by the screen on the host that settles the fleet: the newest
//! completed run on a head's ancestry decides whether that head is tripped (trip.rs, review_hold.rs `Screen`).

use north_base::foundation::main_run_verdict;
use serde::Deserialize;

pub(crate) use main_run_verdict::{Kept, Verdict, failed_cases, judge};

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

type RunKept = Box<dyn Fn(&str) -> Result<Kept, String>>;
type RunPage = Box<dyn Fn(u32) -> Result<Option<Vec<MainRun>>, String>>;

pub(crate) struct MainRuns {
    pub page: RunPage,
    pub kept: RunKept,
    pub alert: Box<dyn Fn(&str)>,
}

impl MainRuns {
    pub(crate) fn main() -> Self {
        Self { page: Box::new(page), kept: Box::new(|run| main_run_verdict::kept(REPOSITORY, run)), alert: Box::new(super::alert::alert_proxy) }
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

/// One page of main's completed Buck2 runs, newest first, the cancelled ones left out; `None` past the last run.
fn page(number: u32) -> Result<Option<Vec<MainRun>>, String> {
    let endpoint = format!("repos/{REPOSITORY}/actions/workflows/{WORKFLOW}/runs?branch=main&status=completed&per_page={PER_PAGE}&page={number}");
    let output = main_run_verdict::gh(&["api", endpoint.as_str(), "--jq", ".workflow_runs"])?;
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

