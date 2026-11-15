//! The one verdict on a red hosted run of main's Buck2 workflow (north#1115, north#1576): the runtime's trip screen and the
//! landing watch's red-main page both read it, so a run that only timed out trips nothing and pages nothing unless one case keeps timing out (north#1537).
use std::collections::BTreeSet;

use serde::Deserialize;


use super::check_log::log_text;

const UNFINISHED: [&str; 2] = ["timed_out", "cancelled"];
const CUT_OFF: [&str; 2] = ["lost communication with the server", "exceeded the maximum execution time"];

/// What GitHub kept of a red run: its job record's word that a job was cut off before it finished, or its failed steps' log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    CutOff(String),
    Log(String),
}

/// How many consecutive red runs of different heads one case must time out or fail in before its timeout is a fault of main rather
/// than of the runner: a guess, settled by counting how often a case that timed out on that many runs in a row passed on the next.
pub const REPEATED: usize = 3;

/// What a completed run says about its head: nothing is wrong, a fault was found with the cases it names, or the run ended without judging the commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Red(Vec<(String, String)>),
    NotJudged(String),
}

impl Verdict {
    pub fn pages(&self) -> bool {
        matches!(self, Self::Red(_))
    }
}

/// The verdict on a run whose conclusion is `red` or not; the run's kept record is read only for a red one, and a read that fails is an error, so nothing is judged from it.
/// A run that only timed out is still red, naming the case, when a case it timed out on also timed out or failed on each of the
/// `REPEATED - 1` red runs of other heads just before it: `earlier` reads their kept records, newest first, and only then.
pub fn judge(red: bool, kept: impl FnOnce() -> Result<Kept, String>, earlier: impl FnOnce(usize) -> Result<Vec<Kept>, String>) -> Result<Verdict, String> {
    if !red {
        return Ok(Verdict::Green);
    }
    let kept = kept()?;
    let Some(why) = not_judged(&kept) else { return Ok(Verdict::Red(failed_cases(log_of(&kept)))) };
    let timed_out = timed_out_cases(log_of(&kept));
    let repeated = if timed_out.is_empty() { timed_out } else { repeated(timed_out, &earlier(REPEATED - 1)?) };
    Ok(if repeated.is_empty() { Verdict::NotJudged(why) } else { Verdict::Red(repeated) })
}

fn log_of(kept: &Kept) -> &str {
    match kept {
        Kept::Log(log) => log,
        Kept::CutOff(_) => "",
    }
}

/// The cases of `cases` that also timed out or failed on every run of `earlier`, none when fewer than `REPEATED - 1` runs came before.
fn repeated(mut cases: Vec<(String, String)>, earlier: &[Kept]) -> Vec<(String, String)> {
    if earlier.len() < REPEATED - 1 {
        return Vec::new();
    }
    for run in earlier {
        let stuck: Vec<(String, String)> = timed_out_cases(log_of(run)).into_iter().chain(failed_cases(log_of(run))).collect();
        cases.retain(|case| stuck.contains(case));
    }
    cases
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

/// What GitHub kept of red run `run` of `repository`: the failed steps' log once a job failed on its own, else why a job was cut off (it
/// timed out, was cancelled, or its failure annotation says its runner was lost or its time limit reached); a read that fails
/// is an error, so nothing is judged from it.
pub fn kept(repository: &str, run: &str) -> Result<Kept, String> {
    let endpoint = format!("repos/{repository}/actions/runs/{run}/jobs");
    let jobs: Vec<Job> = serde_json::from_str(&gh(&["api", endpoint.as_str(), "--jq", ".jobs"])?).map_err(|error| format!("gh api jobs of run {run} is unreadable: {error}"))?;
    let mut cut_off = None;
    for job in jobs {
        let conclusion = job.conclusion.unwrap_or_default();
        let why = match conclusion.as_str() {
            "failure" => cut_off_annotation(repository, job.id)?,
            unfinished if UNFINISHED.contains(&unfinished) => Some(format!("its job concluded {unfinished}")),
            _ => continue,
        };
        match why {
            Some(why) => cut_off = Some(why),
            None => return failed_log(repository, run),
        }
    }
    cut_off.map_or_else(|| failed_log(repository, run), |why| Ok(Kept::CutOff(why)))
}

fn cut_off_annotation(repository: &str, job: u64) -> Result<Option<String>, String> {
    let endpoint = format!("repos/{repository}/check-runs/{job}/annotations");
    let annotations: Vec<Annotation> = serde_json::from_str(&gh(&["api", endpoint.as_str()])?).map_err(|error| format!("gh api annotations of job {job} is unreadable: {error}"))?;
    Ok(annotations.into_iter().find(|annotation| annotation.annotation_level == "failure" && CUT_OFF.iter().any(|cut| annotation.message.contains(cut))).map(|annotation| annotation.message))
}

fn failed_log(repository: &str, run: &str) -> Result<Kept, String> {
    gh(&["run", "view", run, "-R", repository, "--log-failed"]).map(Kept::Log)
}

pub fn gh(arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("gh").args(arguments).output().map_err(|error| format!("gh {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("gh {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The failed crates and cases of a run's log: the nextest `FAIL [ TIME] (N/M) BINARY CASE` lines, each line read past its GitHub stamp and step.
pub fn failed_cases(log: &str) -> Vec<(String, String)> {
    cases(log, "FAIL [")
}

/// The crates and cases a run's log names as timed out: the nextest `TIMEOUT [ TIME] (N/M) BINARY CASE` lines.
pub fn timed_out_cases(log: &str) -> Vec<(String, String)> {
    cases(log, "TIMEOUT [")
}

fn cases(log: &str, marker: &str) -> Vec<(String, String)> {
    let mut cases = BTreeSet::new();
    for line in log.lines() {
        let text = log_text(line);
        let text = match text.strip_prefix('[').and_then(|rest| rest.split_once("] ")) {
            Some((_, rest)) => rest.trim(),
            None => text,
        };
        let Some((took, rest)) = text.strip_prefix(marker).and_then(|rest| rest.split_once(']')) else { continue };
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
pub fn not_judged(kept: &Kept) -> Option<String> {
    match kept {
        Kept::CutOff(why) => Some(format!("its job was cut off before it finished: {why}")),
        Kept::Log(log) => {
            let timed_out = log.lines().map(log_text).any(|text| text.contains("TIMEOUT [") || text.contains("Timeout: "));
            (timed_out && failed_cases(log).is_empty()).then(|| "its cases and targets only timed out, none failed".to_owned())
        }
    }
}

#[cfg(test)]
#[path = "main_run_verdict_tests.rs"]
mod tests;
