//! Buck2's own record of a test run, as `buck2 log show` prints it: each test target's `TestEnd` names the action that
//! ran it and whether its result came from the action cache, and its `TestResult` carries the verdict, the time the
//! run took and what it printed (docs/rationale/compile-reuse.md, Tests).
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;
use std::path::Path;
use std::process::Stdio;

use serde_json::Value;

use super::Buck2;
use crate::{BuildSystem, Cases, Resolution, Selection, Shell, Step, TestRun, Tests};

const TEST: &str = "buck2 test";
const SHOW: &str = "buck2 log show";
const ENDED: &str = "\"TestEnd\"";
const RESULT: &str = "\"TestResult\"";
const PASS: u64 = 1;
/// The case runner's closing line (crates/north-test-cases/src/report.rs).
const SUMMARY: &str = "Summary [";
const RAN: &str = "PASS";
const FAILED: &str = "FAIL";
const SIGNAL: &str = "SIG";

/// The command that prints the record of the run `test` made: `buck2 log show` behind whatever shell `test` ran its `buck2 test` in.
pub fn command(test: &str) -> Option<String> {
    test.split_once(TEST).map(|(shell, _)| format!("{shell}{SHOW}"))
}

fn label(target: &Value) -> Option<String> {
    Some(format!("{}:{}", target["label"]["package"].as_str()?, target["label"]["name"].as_str()?))
}

/// The action that ran a test target and whether its result came from the action cache: only a remote command says so, a local one is never reused.
fn action(ended: &Value) -> Option<(String, (String, bool))> {
    let command = &ended["command_report"]["details"]["command_kind"]["command"];
    let ran = command.get("RemoteCommand").or_else(|| command.get("LocalCommand"))?;
    Some((label(&ended["suite"]["target_label"])?, (ran["action_digest"].as_str()?.to_owned(), ran["cache_hit"] == true)))
}

fn run(result: &Value) -> Result<TestRun, String> {
    let target = label(&result["target_label"]).ok_or("Buck2's event log holds a test result that names no target")?;
    #[allow(clippy::cast_precision_loss)]
    let seconds = result["duration_us"].as_u64().unwrap_or(0) as f64 / 1e6;
    Ok(TestRun { target, action: None, reused: false, passed: result["status"] == PASS, seconds, printed: result["details"].as_str().unwrap_or_default().to_owned() })
}

impl TestRun {
    /// What the case runner's closing line in [`TestRun::printed`] counts; `None` when it printed none.
    pub fn cases(&self) -> Option<Cases> {
        self.printed.lines().rev().find_map(summary)
    }

    /// Each case the case runner ran, as `BINARY-ID CASE`, whatever its verdict.
    pub fn ran(&self) -> impl Iterator<Item = String> + '_ {
        self.printed.lines().filter_map(case)
    }
}

fn summary(line: &str) -> Option<Cases> {
    let (_, counts) = line.trim_start().strip_prefix(SUMMARY)?.split_once(']')?;
    let (run, counts) = counts.trim().split_once(" tests run: ")?;
    let (passed, counts) = counts.split_once(" passed, ")?;
    let (failed, skipped) = counts.split_once(" failed, ")?;
    skipped.strip_suffix(" skipped")?.parse::<u64>().ok()?;
    let (run, passed, failed): (u64, u64, u64) = (run.parse().ok()?, passed.parse().ok()?, failed.parse().ok()?);
    (passed.checked_add(failed) == Some(run)).then_some(Cases { run, failed })
}

/// A case line of the runner: `PASS [ T s] BINARY-ID CASE`, with `FAIL` or the signal that ended the case in place of `PASS`.
fn case(line: &str) -> Option<String> {
    let (status, rest) = line.trim_start().split_once(' ')?;
    if !(status == RAN || status == FAILED || status.starts_with(SIGNAL)) {
        return None;
    }
    let (_, named) = rest.trim_start().strip_prefix('[')?.split_once(']')?;
    let mut words = named.split_whitespace();
    let (binary, case) = (words.next()?, words.next()?);
    words.next().is_none().then(|| format!("{binary} {case}"))
}

/// The cases Buck2 executes when it tests crate `name` in the checkout at `root`, read from its record of that run.
pub(crate) fn executed(root: &Path, name: &str) -> Result<BTreeSet<String>, String> {
    let request = Tests { root, step: Step::Run { doctests: false, keep_going: false }, selection: Selection::Package(name.to_owned()), resolution: Resolution::PINNED, shell: Shell::LandShell, output: None, uncached: false, coverage: None };
    let test = Buck2 { tests: true }.tests(&request);
    let line = test.words.join(" ");
    test.command().stdin(Stdio::null()).stdout(std::io::stderr()).status().map_err(|error| format!("{line}: {error}"))?;
    let record = crate::test_record(&line).ok_or_else(|| format!("{line} keeps no record of its run"))?;
    Ok(record.runs(root)?.iter().filter(|run| super::holds(name, &run.target)).flat_map(TestRun::ran).collect())
}

pub fn runs(record: &mut dyn BufRead) -> Result<Vec<TestRun>, String> {
    let mut actions: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut runs = Vec::new();
    for line in record.lines() {
        let line = line.map_err(|error| format!("cannot read Buck2's event log: {error}"))?;
        if !line.contains(ENDED) && !line.contains(RESULT) {
            continue;
        }
        let event: Value = serde_json::from_str(&line).map_err(|error| format!("Buck2's event log holds a line that is not JSON: {error}"))?;
        let data = &event["Event"]["data"];
        actions.extend(data["SpanEnd"]["data"].get("TestEnd").and_then(action));
        if let Some(result) = data["Instant"]["data"].get("TestResult") {
            runs.push(run(result)?);
        }
    }
    for run in &mut runs {
        if let Some((action, reused)) = actions.remove(&run.target) {
            (run.action, run.reused) = (Some(action), reused);
        }
    }
    Ok(runs)
}

#[cfg(test)]
#[path = "test_record_tests.rs"]
mod tests;
