use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::platform::rolling_issue::{Tracker, settle};

type Verdict = Result<(), String>;

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

pub(super) fn title(host: &str) -> String {
    format!("Harness services failing on {host}")
}

pub(super) fn reconcile(tracker: &mut dyn Tracker, host: &str, failing: &[(String, String)]) -> Result<(), String> {
    let body = (!failing.is_empty()).then(|| body(host, failing));
    settle(tracker, &title(host), body.as_deref(), &format!("Every harness service on {host} finished its latest run; `north health` closed this issue."))?;
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
        "Category: debugging-unknown-cause\nClass: a harness unit failing on {host}\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test\nNeeds: {needs}\n\n\
         The one rolling issue `north health` keeps for {host}. It lists the services failing now, edits itself as they change and closes itself when none fail.\n\n\
         {lines}\n## Done when\n- [ ] (headless run) `north health` on {host} reports no failing unit; this issue then closes itself.\n"
    )
}
