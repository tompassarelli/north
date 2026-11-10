use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::foundation::check_log::{self, Failure};

mod coverage_record;

#[derive(Deserialize)]
struct ActionsRun {
    #[serde(rename = "databaseId")]
    id: u64,
    #[serde(rename = "headSha")]
    sha: String,
    conclusion: Conclusion,
    #[serde(rename = "createdAt")]
    created: String,
}

pub struct Run {
    pub source: Source,
    pub sha: String,
    pub conclusion: Conclusion,
    pub created: String,
}

pub enum Source {
    Actions(u64),
    Coverage(Vec<String>),
}

impl std::fmt::Display for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Actions(id) => write!(formatter, "run {id}"),
            Source::Coverage(_) => formatter.write_str("the coverage record"),
        }
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Conclusion {
    Success,
    Failure,
    #[serde(other)]
    Other,
}

pub struct Window {
    pub since: String,
    pub until: Option<String>,
}

impl Window {
    fn created(&self) -> String {
        self.until.as_ref().map_or_else(|| format!(">={}", self.since), |until| format!("{}..{until}", self.since))
    }
}

pub fn gathered(repo: &Path, window: &Window, coverage: Option<PathBuf>) -> Result<Vec<Run>, String> {
    let mut runs = completed_runs(repo, window)?;
    let named = coverage.is_some();
    let coverage = coverage.unwrap_or_else(|| coverage_record::default_path(repo));
    runs.extend(coverage_record::run_in(&coverage, window, named)?);
    runs.sort_by(|left, right| left.created.cmp(&right.created));
    Ok(runs)
}

pub fn failed_tests(repo: &Path, source: &Source) -> Vec<String> {
    match source {
        Source::Actions(run) => log(repo, *run).map(|text| check_log::failures(&text).into_iter().filter_map(|failure| if let Failure::Cargo(name) = failure { Some(name) } else { None }).collect()).unwrap_or_default(),
        Source::Coverage(tests) => tests.clone(),
    }
}

fn completed_runs(repo: &Path, window: &Window) -> Result<Vec<Run>, String> {
    let output = Command::new("gh").args(["run", "list", "--branch", "main", "--workflow", "Cargo", "--status", "completed", "--limit", "1000", "--json", "databaseId,headSha,conclusion,createdAt", "--created", &window.created()]).current_dir(repo).stdin(Stdio::null()).output().map_err(|why| format!("gh run list: {why}"))?;
    if !output.status.success() {
        return Err(format!("gh run list: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let runs: Vec<ActionsRun> = serde_json::from_slice(&output.stdout).map_err(|why| format!("gh run list: {why}"))?;
    Ok(runs.into_iter().filter(|run| run.conclusion != Conclusion::Other).map(|run| Run { source: Source::Actions(run.id), sha: run.sha, conclusion: run.conclusion, created: run.created }).collect())
}

fn log(repo: &Path, run: u64) -> Option<String> {
    let output = Command::new("gh").args(["run", "view", &run.to_string(), "--log-failed"]).current_dir(repo).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
