//! `north land crate-tests -- COMMAND`: runs a land check's nextest line on the host that executes it. A host whose
//! `[hosts.NAME] buck2_test_crates` lists crates runs the tests of every selected crate it lists in one Buck2 run, and
//! the declared line for the rest of the selection (north#1115). Buck2 fails the check when its build failed or its
//! summary counts a failed test. It passes a crate only when its run passed and, in Buck2's own record of the run, the
//! case runners of that crate's targets count at least one case run in all and none failed. That summary counts a
//! target as one test however many cases it ran, so it never passes a crate alone (north#1299). Every other crate stays
//! on the declared line, so Cargo tests it. The checks before a landing alone wrap their line in this, and key the
//! verdict apart, so the run after a landing tests the landed tree with Cargo itself.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use north_build_system::{Cases, TestRun};

use super::affected::PACKAGES_ENV;
use super::unit_reuse::quote;

const USAGE: &str = "north land crate-tests available | north land crate-tests -- COMMAND";
const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
/// Buck2's exit statuses for a build the tree's own targets or code failed, and for a test run that did not pass.
const BUILD_FAILED: i32 = 3;
const TESTS_FAILED: i32 = 32;
/// The line `buck2 test` ends on: `Tests finished: Pass 1. Fail 0. Timeout 0. ...`, each count possibly coloured.
const SUMMARY: &str = "Tests finished:";
const FAILED: u8 = 1;
/// Keys a nextest verdict that Buck2 may have decided part of apart from the one the run after a landing looks up.
pub const BEFORE_LANDING: (&str, &str) = ("NORTH_LAND_TEST", "buck2-before-landing");

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// The run failed the tree.
    Failed,
    /// The crates Buck2 passed; the rest stay on the declared line.
    Passed(BTreeSet<String>),
}

/// Whether `command` is the nextest line a host's `buck2_test_crates` may split.
pub fn wraps(command: &str) -> bool {
    north_build_system::selects_packages(command, PACKAGES_ENV)
}

/// Whether the verdict of `command`, run before a landing with `selected` packages, may be Buck2's for part of it.
pub fn decides(command: &str, selected: Option<&str>) -> bool {
    wraps(command) && north_build_system::buck2_may_test(selected)
}

/// `command` handed to the executing host's `north`, or run as it is where that north has no such verb.
pub fn wrapped(north: &str, command: &str) -> String {
    format!("n={north}; if \"$n\" land crate-tests available >/dev/null 2>&1; then exec \"$n\" land crate-tests -- {}; fi\n{command}", quote(command))
}

/// How many tests the summary counts under `label`.
fn counted(summary: &str, label: &str) -> Option<u64> {
    let digits: String = summary.split_once(&format!("{label} "))?.1.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The crates whose targets in `runs`, as `owner` assigns them, each passed and carry the case runner's summary, together counting at least one case run and none failed.
fn passed(runs: &[TestRun], owner: &dyn Fn(&str) -> Option<String>) -> BTreeSet<String> {
    let mut summed: BTreeMap<String, Option<Cases>> = BTreeMap::new();
    for run in runs {
        if let Some(name) = owner(&run.target) {
            let sum = summed.entry(name).or_insert(Some(Cases::default()));
            *sum = sum.zip(run.cases().filter(|_| run.passed)).map(|(sum, cases)| Cases { run: sum.run + cases.run, failed: sum.failed + cases.failed });
        }
    }
    summed.into_iter().filter(|(_, cases)| cases.is_some_and(|cases| cases.run > 0 && cases.failed == 0)).map(|(name, _)| name).collect()
}

/// What Buck2 said of the crates of one run, from its exit status, its summary line and its own record of the run, whose targets `owner` assigns to the crates. The record counts only when it holds a run for each target the summary counts as passed.
fn verdict(status: Option<i32>, summary: Option<&str>, runs: Option<&[TestRun]>, owner: &dyn Fn(&str) -> Option<String>) -> Verdict {
    let counts = |label| summary.and_then(|summary| counted(summary, label));
    let recorded = runs.filter(|runs| u64::try_from(runs.len()).ok() == counts("Pass"));
    match (status, counts("Fail"), recorded) {
        (Some(0), Some(0), Some(runs)) => Verdict::Passed(passed(runs, owner)),
        (Some(BUILD_FAILED), ..) => Verdict::Failed,
        (Some(TESTS_FAILED), Some(failed), _) if failed > 0 => Verdict::Failed,
        _ => Verdict::Passed(BTreeSet::new()),
    }
}

/// Runs `line` with its output passed through, and returns its exit status and the last summary line it printed.
fn observed(line: &str) -> (Option<i32>, Option<String>) {
    let Ok(mut child) = Command::new("bash").args(["-euo", "pipefail", "-c", &format!("{{ {line}\n}} 2>&1")]).stdout(Stdio::piped()).spawn() else { return (None, None) };
    let mut summary = None;
    for printed in child.stdout.take().map(BufReader::new).into_iter().flat_map(BufRead::lines).map_while(Result::ok) {
        eprintln!("{printed}");
        if printed.contains(SUMMARY) {
            summary = Some(printed);
        }
    }
    (child.wait().ok().and_then(|status| status.code()), summary)
}

fn declared_line(declared: &str, packages: Option<&str>) -> u8 {
    let mut command = Command::new("bash");
    command.args(["-euo", "pipefail", "-c", declared]);
    if let Some(packages) = packages {
        command.env(PACKAGES_ENV, packages);
    }
    command.status().ok().and_then(|status| status.code()).and_then(|code| u8::try_from(code).ok()).unwrap_or(FAILED)
}

fn execute(declared: &str) -> u8 {
    let variable = |name| std::env::var(name).ok().filter(|value: &String| !value.is_empty());
    let (base, selected) = (variable(BASE_ENV), variable(PACKAGES_ENV));
    let split = std::env::current_dir().ok().and_then(|root| north_build_system::crate_tests(&root, base.as_deref(), declared, PACKAGES_ENV, selected.as_deref()));
    let Some(split) = split else { return declared_line(declared, None) };
    eprintln!("north land crate-tests: this host tests {} with Buck2: {}", split.moved.join(", "), split.line);
    let (status, summary) = observed(&split.line);
    let record = north_build_system::test_record(&split.line).filter(|_| status == Some(0)).and_then(|record| record.runs(Path::new(".")).map_err(|why| eprintln!("north land crate-tests: cannot read Buck2's record of the run: {why}")).ok());
    let owner = |label: &str| split.owner(label).map(str::to_owned);
    let Verdict::Passed(passed) = verdict(status, summary.as_deref(), record.as_deref(), &owner) else { return FAILED };
    for name in split.moved.iter().filter(|name| !passed.contains(*name)) {
        let cases: u64 = record.iter().flatten().filter(|run| split.owner(&run.target) == Some(name.as_str())).filter_map(TestRun::cases).map(|cases| cases.run).sum();
        eprintln!("north land crate-tests: Buck2 gave no verdict on {name} (exit {}, {cases} case(s) of it in its record), so the declared line tests it", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string()));
    }
    split.left(&passed).map_or(0, |packages| declared_line(declared, Some(&packages)))
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb] if verb == "available" => 0,
        [separator, command] if separator == "--" => execute(command),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}

#[cfg(test)]
#[path = "listed_crates_tests.rs"]
mod tests;
