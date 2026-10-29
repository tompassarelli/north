use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::platform::github;

type Verdict = Result<(), String>;

const REPO: &str = "tompassarelli/north";
const IMMEDIATE: [&str; 1] = ["code-health-review.service"];

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Run {
    invocation: String,
    streak: u32,
}

pub(super) struct Reading {
    loaded: bool,
    active: String,
    result: String,
    invocation: String,
}

impl Reading {
    pub(super) fn parse(show: &str) -> Self {
        let field = |key: &str| {
            show.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or_default()
                .trim()
                .to_owned()
        };
        Self {
            loaded: field("LoadState") == "loaded",
            active: field("ActiveState"),
            result: field("Result"),
            invocation: field("InvocationID"),
        }
    }
}

pub(super) fn service_of(timer: &str) -> String {
    format!("{}.service", timer.strip_suffix(".timer").unwrap_or(timer))
}

fn limit(unit: &str) -> u32 {
    if IMMEDIATE.contains(&unit) { 1 } else { 2 }
}

pub(super) fn judge(unit: &str, reading: Option<&Reading>, runs: &mut BTreeMap<String, Run>) -> Verdict {
    let Some(reading) = reading.filter(|reading| reading.loaded) else {
        runs.remove(unit);
        return Ok(());
    };
    let run = runs.entry(unit.into()).or_default();
    if reading.invocation != run.invocation {
        if reading.active == "failed" {
            run.streak += 1;
            run.invocation.clone_from(&reading.invocation);
        } else if reading.active == "inactive" && reading.result == "success" {
            run.streak = 0;
            run.invocation.clone_from(&reading.invocation);
        }
    }
    if run.streak >= limit(unit) {
        Err(format!("{unit} failed {} runs in a row", run.streak))
    } else {
        Ok(())
    }
}

pub(super) struct Issue {
    pub(super) number: u64,
    pub(super) body: String,
}

pub(super) trait Tracker {
    fn open(&self, title: &str) -> Result<Option<Issue>, String>;
    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn edit(&mut self, number: u64, body: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
}

pub(super) fn title(host: &str) -> String {
    format!("Harness services failing on {host}")
}

pub(super) fn reconcile(tracker: &mut dyn Tracker, host: &str, failing: &[(String, String)]) -> Result<(), String> {
    let title = title(host);
    match (tracker.open(&title)?, failing.is_empty()) {
        (None, true) => {}
        (Some(issue), true) => tracker.close(issue.number, &format!("Every harness service on {host} finished its latest run; `north health` closed this issue."))?,
        (None, false) => tracker.create(&title, &body(host, failing))?,
        (Some(issue), false) => {
            let body = body(host, failing);
            if issue.body.trim() != body.trim() {
                tracker.edit(issue.number, &body)?;
            }
        }
    }
    for (number, legacy) in tracker.listed("Harness unit in:title")? {
        if let Some(unit) = legacy_unit(&legacy, host).filter(|unit| !failing.iter().any(|(name, _)| name == unit)) {
            tracker.close(number, &format!("{unit} does not fail on {host} now; `north health` keeps failing services in one rolling issue per host and closed this per-unit issue."))?;
        }
    }
    Ok(())
}

fn legacy_unit<'a>(title: &'a str, host: &str) -> Option<&'a str> {
    let rest = title.strip_prefix("Harness unit ")?;
    rest.strip_suffix(&format!(" keeps failing on {host}")).or_else(|| rest.strip_suffix(" keeps failing"))
}

fn body(host: &str, failing: &[(String, String)]) -> String {
    let needs = if host == "whiterabbit" { "laptop" } else { "local-state" };
    let lines: String = failing.iter().map(|(unit, line)| format!("- `{unit}`: {line}. Read it on {host} with `journalctl --user -u {unit}`.\n")).collect();
    format!(
        "Category: debugging-unknown-cause\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test\nNeeds: {needs}\n\n\
         The one rolling issue `north health` keeps for {host}. It lists the services failing now, edits itself as they change and closes itself when none fail.\n\n\
         {lines}\n## Done when\n- [ ] (headless run) `north health` on {host} reports no failing unit; this issue then closes itself.\n"
    )
}

pub(super) struct Gh;

impl Tracker for Gh {
    fn open(&self, title: &str) -> Result<Option<Issue>, String> {
        let search = format!("in:title \"{title}\"");
        let found = github::json(&["issue", "list", "-R", REPO, "--state", "open", "--search", &search, "--json", "number,title,body"])?;
        Ok(found.as_array().into_iter().flatten().find(|issue| issue["title"] == title).and_then(|issue| {
            Some(Issue { number: issue["number"].as_u64()?, body: issue["body"].as_str().unwrap_or_default().to_owned() })
        }))
    }

    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String> {
        let found = github::json(&["issue", "list", "-R", REPO, "--state", "open", "--search", search, "--json", "number,title"])?;
        Ok(found.as_array().into_iter().flatten().filter_map(|issue| Some((issue["number"].as_u64()?, issue["title"].as_str()?.to_owned()))).collect())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "create", "-R", REPO, "--title", title, "--body", body]), "issue create")
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "edit", &number.to_string(), "-R", REPO, "--body", body]), "issue edit")
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "close", &number.to_string(), "-R", REPO, "--comment", comment]), "issue close")
    }
}

fn succeeded(out: std::io::Result<std::process::Output>, what: &str) -> Result<(), String> {
    let out = out.map_err(|error| format!("gh {what}: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("gh {what}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
