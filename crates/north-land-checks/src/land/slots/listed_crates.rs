//! `north land crate-tests -- COMMAND`: runs a land check's nextest line on the host that executes it. A host whose
//! `[hosts.NAME] buck2_test_crates` lists crates runs the tests of each selected crate it lists through Buck2, one crate
//! at a time, and the declared line for the rest of the selection (north#1115). Buck2 decides for a crate only when it
//! says so plainly: it passed at least one test and failed none, its build failed, or its summary counts a failed test.
//! On anything else the crate stays on the declared line, so Cargo tests it. The checks before a landing alone wrap
//! their line in this, and key the verdict apart, so the run after a landing tests the landed tree with Cargo itself.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

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
    Passed,
    Failed,
    Silent,
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

/// What Buck2 said of one crate, from its exit status and its summary line.
fn verdict(status: Option<i32>, summary: Option<&str>) -> Verdict {
    let counts = |label| summary.and_then(|summary| counted(summary, label));
    match (status, counts("Pass"), counts("Fail")) {
        (Some(0), Some(passed), Some(0)) if passed > 0 => Verdict::Passed,
        (Some(BUILD_FAILED), ..) => Verdict::Failed,
        (Some(TESTS_FAILED), _, Some(failed)) if failed > 0 => Verdict::Failed,
        _ => Verdict::Silent,
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
    let mut passed = BTreeSet::new();
    for (name, line) in &split.moved {
        eprintln!("north land crate-tests: this host tests {name} with Buck2: {line}");
        let (status, summary) = observed(line);
        match verdict(status, summary.as_deref()) {
            Verdict::Passed => {
                passed.insert(name.clone());
            }
            Verdict::Failed => return FAILED,
            Verdict::Silent => eprintln!("north land crate-tests: Buck2 gave no verdict on {name} (exit {}), so the declared line tests it", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string())),
        }
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
