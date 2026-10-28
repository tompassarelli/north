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

pub(super) trait Tracker {
    fn open(&self, title: &str) -> Result<Option<u64>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn comment(&mut self, number: u64, body: &str) -> Result<(), String>;
}

pub(super) fn title(unit: &str, host: &str) -> String {
    format!("Harness unit {unit} keeps failing on {host}")
}

pub(super) fn report(tracker: &mut dyn Tracker, unit: &str, host: &str, line: &str) -> Result<(), String> {
    let title = title(unit, host);
    match tracker.open(&title)? {
        Some(number) => tracker.comment(number, line),
        None => tracker.create(&title, &body(unit, host, line)),
    }
}

fn body(unit: &str, host: &str, line: &str) -> String {
    format!(
        "Category: debugging-unknown-cause\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test\n\n\
         `north health` on {host} ({line}). Read the failure on {host} with `journalctl --user -u {unit}`.\n\n\
         ## Done when\n- [ ] (headless run) The journal of `{unit}` on {host} ends in `Finished` on its next timer run and `north health` reports no failing unit.\n"
    )
}

pub(super) struct Gh;

impl Tracker for Gh {
    fn open(&self, title: &str) -> Result<Option<u64>, String> {
        let search = format!("in:title \"{title}\"");
        let found = github::json(&["issue", "list", "-R", REPO, "--state", "open", "--search", &search, "--json", "number,title"])?;
        Ok(found
            .as_array()
            .into_iter()
            .flatten()
            .find(|issue| issue["title"] == title)
            .and_then(|issue| issue["number"].as_u64()))
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "create", "-R", REPO, "--title", title, "--body", body]), "issue create")
    }

    fn comment(&mut self, number: u64, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "comment", &number.to_string(), "-R", REPO, "--body", body]), "issue comment")
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
