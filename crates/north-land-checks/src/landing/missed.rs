use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::foundation::check_log::{self, Failure};
use crate::land::slots::graph_selection::{SELECTED, Selected};
use crate::land::slots::guard_suites::Suites;
use crate::platform::records_mirror;

const USAGE: &str = "usage: north land missed --since DATE [--repo DIR]
lists each failing Cargo run on main created at or after DATE (gh's --created form, such as 2026-11-09T01:00:00Z), its failed tests, and the ones the change graph's after-landing selection for the landing range holding that commit left out; exits 1 when one was left out";

#[derive(Deserialize)]
struct Run {
    #[serde(rename = "databaseId")]
    id: u64,
    #[serde(rename = "headSha")]
    sha: String,
}

struct Landing {
    base: String,
    head: String,
    selected: Selected,
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut since, mut repo) = (None, PathBuf::from("."));
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--since", Some(value)) => since = Some(value.clone()),
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
    let records = records_mirror::read("north land missed");
    if let Some(why) = refused(&records, &since, records_mirror::now()) {
        eprintln!("north land missed: {why}");
        return 1;
    }
    let landings = landings(&records.home().join("north/land-queue"));
    let suites = Suites::read(&repo).ok();
    let runs = match failed_runs(&repo, &since) {
        Ok(runs) => runs,
        Err(why) => {
            eprintln!("north land missed: {why}");
            return 1;
        }
    };
    let (mut judged, mut missed, mut unjudged) = (0, 0, 0);
    for run in &runs {
        let tests: Vec<String> = log(&repo, run.id).map(|text| check_log::failures(&text).into_iter().filter_map(|failure| if let Failure::Cargo(name) = failure { Some(name) } else { None }).collect()).unwrap_or_default();
        if tests.is_empty() {
            println!("run {} at {}: its log names no failed test", run.id, short(&run.sha));
            continue;
        }
        let Some(landing) = landings.iter().find(|landing| holds(&repo, landing, &run.sha)) else {
            unjudged += tests.len();
            println!("run {} at {}: {} failed tests, and no after-landing run recorded a change-graph selection for a range holding it: {}", run.id, short(&run.sha), tests.len(), tests.join(", "));
            continue;
        };
        let floor = suites.as_ref().map(|suites| suites.floor(&changed(&repo, &landing.base, &landing.head)));
        let held = |name: &str| landing.selected.holds(name) || floor.as_ref().is_some_and(|floor| floor.selects(name));
        let guarded = tests.iter().map(String::as_str).filter(|name| !landing.selected.holds(name) && floor.as_ref().is_some_and(|floor| floor.selects(name))).count();
        let left: Vec<&str> = tests.iter().map(String::as_str).filter(|name| !held(name)).collect();
        judged += tests.len();
        missed += left.len();
        let detail = if !left.is_empty() {
            format!("; left out: {}", left.join(", "))
        } else if guarded > 0 {
            format!("; {guarded} held by the guard suite")
        } else {
            String::new()
        };
        println!("run {} at {}: {} failed tests, {} in the after-landing selection of {}..{}{detail}", run.id, short(&run.sha), tests.len(), tests.len() - left.len(), short(&landing.base), short(&landing.head));
    }
    println!("north land missed: {} failing Cargo runs on main since {since}; of {judged} failed tests in a range with a recorded selection, {missed} were left out of it; {unjudged} failed in a range with none", runs.len());
    u8::from(missed > 0)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn refused(records: &records_mirror::Records, since: &str, now: f64) -> Option<String> {
    let stamp = records.source()?;
    let age = (now - stamp.at).max(0.0);
    let since = crate::foundation::clock::Timestamp::parse_rfc3339(since).map(|at| at.unix_seconds_f64());
    let before = since.is_some_and(|since| stamp.at < since);
    if !before && age <= records_mirror::FRESH.as_secs_f64() {
        return None;
    }
    let note = records.note(now).unwrap_or_default();
    Some(format!("refusing a stale mirror: {note}; only {} holds the land records this window judges, so run it there", stamp.host))
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

fn changed(repo: &Path, base: &str, head: &str) -> Vec<String> {
    let Ok(output) = Command::new("git").arg("-C").arg(repo).args(["diff", "--name-only", base, head]).stderr(Stdio::null()).output() else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

fn ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    Command::new("git").arg("-C").arg(repo).args(["merge-base", "--is-ancestor", older, newer]).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn holds(repo: &Path, landing: &Landing, sha: &str) -> bool {
    sha != landing.base && ancestor(repo, &landing.base, sha) && ancestor(repo, sha, &landing.head)
}

fn failed_runs(repo: &Path, since: &str) -> Result<Vec<Run>, String> {
    let output = Command::new("gh").args(["run", "list", "--branch", "main", "--workflow", "Cargo", "--status", "failure", "--limit", "200", "--json", "databaseId,headSha", "--created"]).arg(format!(">={since}")).current_dir(repo).stdin(Stdio::null()).output().map_err(|why| format!("gh run list: {why}"))?;
    if !output.status.success() {
        return Err(format!("gh run list: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|why| format!("gh run list: {why}"))
}

fn log(repo: &Path, run: u64) -> Option<String> {
    let output = Command::new("gh").args(["run", "view", &run.to_string(), "--log-failed"]).current_dir(repo).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "missed_tests.rs"]
mod tests;
