use crate::platform::rolling_issue::{Breaches, Tracker, alarm};

type Verdict = Result<(), String>;

const IMMEDIATE: [&str; 1] = ["code-health-review.service"];

pub(super) struct Reading {
    loaded: bool,
    finished: Vec<String>,
}

impl Reading {
    pub(super) fn parse(show: &str, journal: &str) -> Self {
        let loaded = show.lines().any(|line| line.trim() == "LoadState=loaded");
        let finished = journal.lines().map(str::trim).filter(|result| !result.is_empty()).map(str::to_owned).collect();
        Self { loaded, finished }
    }

    fn failed_in_a_row(&self) -> usize {
        self.finished
            .iter()
            .filter(|result| !matches!(result.as_str(), "canceled" | "skipped"))
            .take_while(|result| matches!(result.as_str(), "failed" | "timeout" | "dependency"))
            .count()
    }
}

pub(super) fn service_of(timer: &str) -> String {
    format!("{}.service", timer.strip_suffix(".timer").unwrap_or(timer))
}

fn limit(unit: &str) -> usize {
    if IMMEDIATE.contains(&unit) { 1 } else { 2 }
}

pub(super) fn judge(unit: &str, reading: Option<&Reading>) -> Verdict {
    let Some(reading) = reading.filter(|reading| reading.loaded) else {
        return Ok(());
    };
    let streak = reading.failed_in_a_row();
    if streak >= limit(unit) {
        Err(format!("{unit} failed {streak} runs in a row"))
    } else {
        Ok(())
    }
}

pub(super) fn title(host: &str) -> String {
    format!("Harness services failing on {host}")
}

pub(super) fn reconcile(tracker: &mut dyn Tracker, breaches: &mut Breaches, host: &str, failing: &[(String, String)]) -> Result<(), String> {
    let body = (!failing.is_empty()).then(|| body(host, failing));
    alarm(tracker, breaches, &title(host), body.as_deref(), &format!("Every harness service on {host} finished its latest run; `north health` closed this issue."))?;
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
        "Class: a harness unit failing on {host}\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test\nNeeds: {needs}\n\n\
         The one rolling issue `north health` keeps for {host}. It lists the services failing now, edits itself as they change and closes itself when none fail.\n\n\
         {lines}\n## Done when\n- [ ] (headless run) `north health` on {host} reports no failing unit; this issue then closes itself.\n"
    )
}
