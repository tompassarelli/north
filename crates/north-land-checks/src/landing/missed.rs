use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::foundation::check_log::{self, Failure};
use crate::foundation::clock::Timestamp;
use crate::land::slots::graph_selection::{SELECTED, Selected};
use crate::land::slots::guard_suites::Suites;
use crate::platform::records_mirror;

mod base;
pub use base::classify;
use selection_miss::rev_parse;
mod responsible;
mod selection_miss;


const USAGE: &str = "usage: north land missed --since DATE [--until DATE] [--repo DIR]
lists each failing Cargo run on main created at or after DATE and before --until (gh's --created form, such as 2026-11-09T01:00:00Z), its failed tests, and the ones the change graph's after-landing selection for the landing range holding that commit left out, apart from a test that also failed in the run on that commit's parent, which is listed as an environment failure and is no miss; for each failed test it names the lane that turned it red, and when that lane's own selection did not hold the test it records a selection-miss incident and tells the graph (test :guards FILE) for each file the lane changed; exits 1 when one was left out";
const SETTLED: f64 = 3600.0;

#[derive(Deserialize)]
struct Run {
    #[serde(rename = "databaseId")]
    id: u64,
    #[serde(rename = "headSha")]
    sha: String,
    conclusion: Conclusion,
    #[serde(rename = "createdAt")]
    created: String,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Conclusion {
    Success,
    Failure,
    #[serde(other)]
    Other,
}

struct Landing {
    base: String,
    head: String,
    selected: Selected,
}

struct Window {
    since: String,
    until: Option<String>,
}

impl Window {
    fn created(&self) -> String {
        self.until.as_ref().map_or_else(|| format!(">={}", self.since), |until| format!("{}..{until}", self.since))
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut since, mut until, mut repo) = (None, None, PathBuf::from("."));
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--since", Some(value)) => since = Some(value.clone()),
            ("--until", Some(value)) => until = Some(value.clone()),
            ("--repo", Some(value)) => repo = PathBuf::from(value),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(since) = since else {
        eprintln!("{USAGE}");
        return 2;
    };
    let window = Window { since, until };
    let records = records_mirror::read("north land missed");
    if let Some(why) = refused(&records, &window.since, window.until.as_deref(), records_mirror::now()) {
        eprintln!("north land missed: {why}");
        return 1;
    }
    let landings = landings(&records.home().join("north/land-queue"));
    let suites = Suites::read(&repo).ok();
    let mut runs = match completed_runs(&repo, &window) {
        Ok(runs) => runs,
        Err(why) => {
            eprintln!("north land missed: {why}");
            return 1;
        }
    };
    runs.sort_by(|left, right| left.created.cmp(&right.created));
    let mut outcomes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut tally, mut failing) = (Tally::default(), Vec::new());
    for run in &runs {
        if run.conclusion != Conclusion::Failure {
            outcomes.entry(run.sha.clone()).or_default();
            continue;
        }
        let tests = failed_tests(&repo, run.id);
        outcomes.entry(run.sha.clone()).or_default().extend(tests.iter().cloned());
        failing.push((run, tests.clone()));
        if tests.is_empty() {
            println!("run {} at {}: its log names no failed test", run.id, short(&run.sha));
            continue;
        }
        let on_base = rev_parse(&repo, &format!("{}^", run.sha)).and_then(|parent| outcomes.get(&parent)).map(|failed| failed.iter().cloned().collect::<Vec<_>>());
        tally.add(after_landing(&repo, &landings, suites.as_ref(), run, &tests, on_base.as_deref()));
    }
    let failing_count = failing.len();
    selection_miss::name_lanes(&repo, &runs, failing, &outcomes, &landings, &records.home().join("safe-push/post-land.tsv"));
    println!("north land missed: {failing_count} failing Cargo runs on main since {}; of {} failed tests in a range with a recorded selection, {} were left out of it; {} more also fail on their base commit and are environment failures; {} failed in a range with none", window.since, tally.judged, tally.missed, tally.environment, tally.unjudged);
    u8::from(tally.missed > 0)
}

#[derive(Default)]
struct Tally {
    judged: usize,
    missed: usize,
    unjudged: usize,
    environment: usize,
}

impl Tally {
    fn add(&mut self, other: Self) {
        self.judged += other.judged;
        self.missed += other.missed;
        self.unjudged += other.unjudged;
        self.environment += other.environment;
    }
}

fn failed_tests(repo: &Path, run: u64) -> Vec<String> {
    log(repo, run).map(|text| check_log::failures(&text).into_iter().filter_map(|failure| if let Failure::Cargo(name) = failure { Some(name) } else { None }).collect()).unwrap_or_default()
}

fn after_landing(repo: &Path, landings: &[Landing], suites: Option<&Suites>, run: &Run, tests: &[String], on_base: Option<&[String]>) -> Tally {
    let Some(landing) = landings.iter().find(|landing| holds(repo, landing, &run.sha)) else {
        println!("run {} at {}: {} failed tests, and no after-landing run recorded a change-graph selection for a range holding it: {}", run.id, short(&run.sha), tests.len(), tests.join(", "));
        return Tally { unjudged: tests.len(), ..Tally::default() };
    };
    let floor = suites.map(|suites| suites.floor(&changed(repo, &landing.base, &landing.head)));
    let guards = |name: &str| floor.as_ref().is_some_and(|floor| floor.selects(name));
    let verdict = classify(tests, on_base, |name| landing.selected.holds(name) || guards(name));
    let guarded = verdict.selected.iter().filter(|name| !landing.selected.holds(name)).count();
    let mut detail = String::new();
    if !verdict.left_out.is_empty() {
        detail.push_str(&format!("; left out: {}", verdict.left_out.join(", ")));
    } else if guarded > 0 {
        detail.push_str(&format!("; {guarded} held by the guard suite"));
    }
    if !verdict.environment.is_empty() {
        detail.push_str(&format!("; environment failures that also fail on the base: {}", verdict.environment.join(", ")));
    }
    println!("run {} at {}: {} failed tests, {} in the after-landing selection of {}..{}{detail}", run.id, short(&run.sha), tests.len(), verdict.selected.len(), short(&landing.base), short(&landing.head));
    Tally { judged: verdict.selected.len() + verdict.left_out.len(), missed: verdict.left_out.len(), unjudged: 0, environment: verdict.environment.len() }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn refused(records: &records_mirror::Records, since: &str, until: Option<&str>, now: f64) -> Option<String> {
    let stamp = records.source()?;
    let age = (now - stamp.at).max(0.0);
    let at = |text: &str| Timestamp::parse_rfc3339(text).map(|at| at.unix_seconds_f64());
    let before = at(since).is_some_and(|since| stamp.at < since);
    let settled = until.and_then(at).is_some_and(|until| stamp.at >= until + SETTLED);
    if !before && (settled || age <= records_mirror::FRESH.as_secs_f64()) {
        return None;
    }
    let note = records.note(now).unwrap_or_default();
    Some(format!("refusing a stale mirror: {note}; only {} holds the land records this window judges, so run it there or end the window with --until an hour before the mirror", stamp.host))
}
fn landings(queues: &Path) -> Vec<Landing> {
    let logs = fs::read_dir(queues).into_iter().flatten().flatten().map(|queue| queue.path().join("logs"));
    let outcomes = logs.flat_map(|logs| fs::read_dir(logs).into_iter().flatten().flatten().map(|entry| entry.path())).filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("post-land-")) && path.extension().is_some_and(|extension| extension == "outcome"));
    let mut found = Vec::new();
    for text in outcomes.filter_map(|path| fs::read_to_string(path).ok()) {
        for line in text.lines() {
            let fields: Vec<&str> = line.splitn(4, '\t').collect();
            if let [SELECTED, base, head, json] = fields.as_slice()
                && let Ok(selected) = serde_json::from_str(json)
            {
                found.push(Landing { base: (*base).to_owned(), head: (*head).to_owned(), selected });
            }
        }
    }
    found
}

fn ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    Command::new("git").arg("-C").arg(repo).args(["merge-base", "--is-ancestor", older, newer]).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn holds(repo: &Path, landing: &Landing, sha: &str) -> bool {
    sha != landing.base && ancestor(repo, &landing.base, sha) && ancestor(repo, sha, &landing.head)
}

fn completed_runs(repo: &Path, window: &Window) -> Result<Vec<Run>, String> {
    let output = Command::new("gh").args(["run", "list", "--branch", "main", "--workflow", "Cargo", "--status", "completed", "--limit", "1000", "--json", "databaseId,headSha,conclusion,createdAt", "--created", &window.created()]).current_dir(repo).stdin(Stdio::null()).output().map_err(|why| format!("gh run list: {why}"))?;
    if !output.status.success() {
        return Err(format!("gh run list: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let runs: Vec<Run> = serde_json::from_slice(&output.stdout).map_err(|why| format!("gh run list: {why}"))?;
    Ok(runs.into_iter().filter(|run| run.conclusion != Conclusion::Other).collect())
}

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stderr(Stdio::null()).output();
    output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()).unwrap_or_default()
}

fn changed(repo: &Path, parent: &str, sha: &str) -> Vec<String> {
    git_lines(repo, &["diff", "--name-only", "--no-renames", parent, sha])
}
fn log(repo: &Path, run: u64) -> Option<String> {
    let output = Command::new("gh").args(["run", "view", &run.to_string(), "--log-failed"]).current_dir(repo).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "missed_tests.rs"]
mod tests;
