//! `north land crate-tests -- COMMAND`: runs a land check's nextest line on the host that executes it. A host whose
//! `[hosts.NAME] buck2_test_crates` lists crates runs the tests of every selected crate it lists in one Buck2 run, and
//! the declared line for the rest of the selection (north#1115). Buck2 fails the check when its build failed or its
//! summary counts a failed test. It passes a crate only when its run passed and, in Buck2's own record of the run, the
//! case runners of that crate's targets count at least one case run in all and none failed; a crate the host's
//! `buck2_caseless_crates` names, entered by a comparison that found no case on either side, passes with none run, and
//! is judged as any other the moment one of its cases runs. That summary counts a
//! target as one test however many cases it ran, so it never passes a crate alone (north#1299). Every other crate stays
//! on the declared line, so Cargo tests it. A passed crate's host cases, the cases of the reviewed class in
//! the landing base's `build/host-cases.list` that its Buck2 targets skip because no remote worker can run them, then run here in one run of
//! the declared line narrowed to them, which fails the check when it fails or runs fewer cases than the class names. A
//! host that also sets `buck2_host_cases` runs them instead from the binaries Buck2 builds on the workers, each listed
//! case by name on this host in the declared line's shell, and fails the check unless every case of the class ran and
//! passed; `north land crate-tests host-cases [PACKAGE...]` runs that step alone. The
//! checks before a landing wrap their line in this, and key the verdict apart; the run after a landing wraps it too, and a
//! host splits it only when its `buck2_after_run` is set, else the declared line runs as it is.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use north_build_system::{Cases, CrateTests, TestRun};

use super::affected::PACKAGES_ENV;
use super::it_modules::IT_ENV;
use super::unit_reuse::quote;
use crate::foundation::check_log::{Failure, failures};
use crate::land::trace::check_log;

const USAGE: &str = "north land crate-tests available | north land crate-tests after-run-available | north land crate-tests host-cases [PACKAGE...] | north land crate-tests after-landing | north land crate-tests -- COMMAND";
const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
/// Buck2's exit statuses for a build the tree's own targets or code failed, and for a test run that did not pass.
const BUILD_FAILED: i32 = 3;
const TESTS_FAILED: i32 = 32;
/// The line `buck2 test` ends on: `Tests finished: Pass 1. Fail 0. Timeout 0. ...`, each count possibly coloured.
const SUMMARY: &str = "Tests finished:";
/// The line cargo-nextest ends a run on: `Summary [ T s] N tests run: P passed, ...`, `1 test run` for one case.
const NEXTEST_SUMMARY: &str = "Summary [";
const FAILED: u8 = 1;
/// The line Buck2 prints when a remote worker lost an action, before any test in it ran (north#1115).
const REMOTE_EXECUTION_ERROR: &str = "Remote Execution Error";
/// The end of an action whose deadline ran out, which is the test's own timeout and so a red, not infrastructure (north#1115).
const DEADLINE_EXCEEDED: &str = "deadline exceeded";
/// The incident class a remote worker's lost action is recorded under, apart from the lander's own classes (north#1115).
const INFRASTRUCTURE_CLASS: &str = "remote-execution-infrastructure";
/// The mark Buck2 puts before each target it counts as failed: `  ✗ root//crates/NAME:TARGET`.
const FAILED_MARK: &str = "✗ ";
/// Keys a nextest verdict that Buck2 may have decided part of apart from the one the run after a landing looks up.
pub const BEFORE_LANDING: (&str, &str) = ("NORTH_LAND_TEST", "buck2-before-landing");
pub const AFTER_LANDING: (&str, &str) = ("NORTH_LAND_TEST", "buck2-after-landing");
/// The key entry of the test line of the run after a landing while no host sets `buck2_after_run`: none, so that run keys as it always has.
pub const AFTER_LANDING_RUN: (&str, &str) = ("NORTH_LAND_TEST", "");
/// The run after a landing gives no verdict when Buck2 cannot test the tree here or its record of the run is unreadable.
pub const NO_VERDICT: u8 = 75;
/// Marks each crate the run after a landing passed on its stdout, which the offload also writes its own lines to.
pub const PASSED: &str = "north land crate-tests after-landing passed ";
/// The nextest line the run after a landing splits: only its shell is run, since every crate it moves is tested by Buck2 and the rest are left to the Cargo run after the landing.
const AFTER_LANDING_LINE: &str = "./bin/land-shell --dev -- cargo nextest run ${SAFE_PUSH_LAND_PACKAGES}";

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

/// Whether the verdict of `command`, run before a landing with `selected` packages, or the packages it names itself,
/// may be Buck2's for part of it.
pub fn decides(command: &str, selected: Option<&str>) -> bool {
    let named = north_build_system::named_selection(command, PACKAGES_ENV).map(|(_, named)| named);
    wraps(command) && north_build_system::buck2_may_test(named.as_deref().or(selected))
}

/// `command` handed to the executing host's `north`, or run as it is where that north has no such verb.
pub fn wrapped(north: &str, command: &str) -> String {
    wrapped_by(north, command, "available")
}

/// `command` handed to the executing host's `north` only where its `buck2_after_run` is set, else run as it is: the wrapper of the run after a landing.
pub fn wrapped_after(north: &str, command: &str) -> String {
    wrapped_by(north, command, "after-run-available")
}

fn wrapped_by(north: &str, command: &str, probe: &str) -> String {
    format!("n={north}; if \"$n\" land crate-tests {probe} >/dev/null 2>&1; then exec \"$n\" land crate-tests -- {}; fi\n{command}", quote(command))
}

/// How many tests the summary counts under `label`.
fn counted(summary: &str, label: &str) -> Option<u64> {
    let digits: String = summary.split_once(&format!("{label} "))?.1.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The crates whose targets in `runs`, as `owner` assigns them, each passed and carry the case runner's summary, together counting none failed and at least one case run, or none for a crate `caseless` names.
fn passed(runs: &[TestRun], owner: &dyn Fn(&str) -> Option<String>, caseless: &BTreeSet<String>) -> BTreeSet<String> {
    let mut summed: BTreeMap<String, Option<Cases>> = BTreeMap::new();
    for run in runs {
        if let Some(name) = owner(&run.target) {
            let sum = summed.entry(name).or_insert(Some(Cases::default()));
            *sum = sum.zip(run.cases().filter(|_| run.passed)).map(|(sum, cases)| Cases { run: sum.run + cases.run, failed: sum.failed + cases.failed });
        }
    }
    summed.into_iter().filter(|(name, cases)| cases.is_some_and(|cases| (cases.run > 0 || caseless.contains(name)) && cases.failed == 0)).map(|(name, _)| name).collect()
}

/// What Buck2 said of the crates of one run, from its exit status, its summary line and its own record of the run, whose targets `owner` assigns to the crates. The record counts only when it holds a run for each target the summary counts as passed. A failure `infrastructure` names is no test's, so it gives no verdict and the crates stay on the declared line.
fn verdict(status: Option<i32>, summary: Option<&str>, runs: Option<&[TestRun]>, owner: &dyn Fn(&str) -> Option<String>, caseless: &BTreeSet<String>, infrastructure: bool) -> Verdict {
    let counts = |label| summary.and_then(|summary| counted(summary, label));
    let recorded = runs.filter(|runs| u64::try_from(runs.len()).ok() == counts("Pass"));
    match (status, counts("Fail"), recorded) {
        (Some(0), Some(0), Some(runs)) => Verdict::Passed(passed(runs, owner, caseless)),
        (Some(BUILD_FAILED), ..) => Verdict::Failed,
        (Some(TESTS_FAILED), Some(failed), _) if failed > 0 && !infrastructure => Verdict::Failed,
        _ => Verdict::Passed(BTreeSet::new()),
    }
}

/// The first remote execution error in `output`, when Buck2 counts a failed target, no case of any test is named as
/// failed and no deadline or test timeout ran out: a remote worker cancelled or lost the action before a result, so the
/// failure is infrastructure (north#1115).
fn remote_error(output: &[String]) -> Option<&str> {
    let error = output.iter().map(|line| line.trim()).find(|line| line.contains(REMOTE_EXECUTION_ERROR))?;
    let named = failures(&output.join("\n")).iter().any(|failure| matches!(failure, Failure::Cargo(_)));
    let timed_out = output.iter().any(|line| line.contains(DEADLINE_EXCEEDED) || (line.contains(SUMMARY) && counted(line, "Timeout").is_some_and(|timeouts| timeouts > 0)));
    (!named && !timed_out && !failed_targets(output).is_empty()).then_some(error)
}

/// The targets Buck2 printed as failed, each as `root//crates/NAME:TARGET`.
fn failed_targets(output: &[String]) -> Vec<&str> {
    output.iter().filter_map(|line| line.trim().strip_prefix(FAILED_MARK)).filter(|target| target.starts_with("root//")).collect()
}

/// The commit this checkout is at, for the record of an infrastructure failure.
fn head() -> String {
    Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map_or_else(|| "unknown".to_owned(), |output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The last line of `output` holding `summary`.
fn last_with(output: &[String], summary: &str) -> Option<String> {
    output.iter().rev().find(|printed| printed.contains(summary)).cloned()
}

/// Runs `line` with `environment` set and its output passed through, and returns its exit status and every line it printed.
fn run_printing(line: &str, environment: &[(&str, &str)]) -> (Option<i32>, Vec<String>) {
    let mut command = Command::new("bash");
    command.args(["-euo", "pipefail", "-c", &format!("{{ {line}\n}} 2>&1")]).envs(environment.iter().copied()).stdout(Stdio::piped());
    let Ok(mut child) = command.spawn() else { return (None, Vec::new()) };
    let mut output = Vec::new();
    for printed in child.stdout.take().map(BufReader::new).into_iter().flat_map(BufRead::lines).map_while(Result::ok) {
        eprintln!("{printed}");
        output.push(printed);
    }
    (child.wait().ok().and_then(|status| status.code()), output)
}

/// Runs `line` with `environment` set and its output passed through, and returns its exit status and the last line it printed holding `summary`.
fn observed(line: &str, environment: &[(&str, &str)], summary: &str) -> (Option<i32>, Option<String>) {
    let (status, output) = run_printing(line, environment);
    (status, last_with(&output, summary))
}

/// How many cases nextest's closing line says ran.
fn ran(summary: &str) -> Option<u64> {
    summary.split_once("] ")?.1.split_once(" test")?.0.trim().parse().ok()
}

/// How many cases the case runner's closing line says ran and passed: `Summary [ T s] N tests run: P passed, ...`.
fn ran_and_passed(summary: &str) -> Option<(usize, usize)> {
    let passed = summary.split_once("run: ")?.1.split_once(" passed")?.0.trim().parse().ok()?;
    Some((usize::try_from(ran(summary)?).ok()?, passed))
}

/// Runs the cases of `class`, each binary's on the line `runs` gives for it: the case runner over the binary Buck2
/// built on the workers, run here. Whether every binary has a line, each run passed, and together they ran and passed
/// exactly as many cases as the class names; `Err` when the binaries could not be built or their plans read.
fn built_host_cases(class: &[(String, String)], runs: impl FnOnce(&BTreeMap<String, Vec<String>>) -> Result<BTreeMap<String, Vec<String>>, String>) -> Result<bool, String> {
    let mut cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (binary, case) in class {
        cases.entry(binary.clone()).or_default().push(case.clone());
    }
    eprintln!("north land crate-tests: this host runs the {} host case(s) of {} from the test binaries Buck2 builds", class.len(), cases.keys().cloned().collect::<Vec<_>>().join(", "));
    let lines = runs(&cases)?;
    let (mut ran, mut passed, mut clean) = (0, 0, true);
    for binary in cases.keys() {
        let Some(line) = lines.get(binary) else {
            eprintln!("north land crate-tests: no host-case target of Buck2 plans {binary}, whose host cases the class names");
            clean = false;
            continue;
        };
        let line = line.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" ");
        eprintln!("north land crate-tests: {line}");
        let (status, summary) = observed(&line, &[], NEXTEST_SUMMARY);
        let (run, pass) = summary.as_deref().and_then(ran_and_passed).unwrap_or_default();
        clean &= status == Some(0);
        (ran, passed) = (ran + run, passed + pass);
    }
    eprintln!("north land crate-tests: host cases: {ran} run, {passed} passed, of the {} the class names", class.len());
    Ok(clean && ran == class.len() && passed == ran)
}

/// Runs the host cases of the crates in `passed` that the class names, read from the landing `base` and never from the lane at `root`, on the `declared` line narrowed to them, or from the binaries Buck2 builds when the host's `split` says so; whether there were none, or the run passed and ran as many cases as the class names.
fn host_cases(root: &Path, base: Option<&str>, declared: &str, passed: &BTreeSet<String>, split: &CrateTests) -> Result<bool, String> {
    let package = |binary: &str| binary.split_once("::").map_or(binary, |(package, _)| package).to_owned();
    let class: Vec<(String, String)> = north_buck::host_class::reviewed(root, base)?.into_iter().filter(|(binary, _)| passed.contains(&package(binary))).collect();
    if split.builds_host_cases() && !class.is_empty() {
        return built_host_cases(&class, |cases| split.host_runs(root, north_buck::host_class::TARGET, cases));
    }
    let names: Vec<String> = class.iter().map(|(binary, _)| package(binary)).collect::<BTreeSet<_>>().into_iter().collect();
    let Some(filter) = north_buck::host_class::filter(root, base, &names)?.filter(|_| !names.is_empty()) else { return Ok(true) };
    let tests: BTreeSet<&str> = class.iter().filter_map(|(binary, _)| binary.split_once("::").map(|(_, test)| test)).collect();
    let narrowed = tests.iter().map(|test| format!("--test {test} ")).chain([format!("-E {filter}")]).collect::<String>();
    let packages = names.iter().map(|name| format!("-p {name}")).collect::<Vec<_>>().join(" ");
    eprintln!("north land crate-tests: the declared line runs the {} host case(s) of {}", class.len(), names.join(", "));
    let (status, summary) = observed(declared, &[(PACKAGES_ENV, &packages), (IT_ENV, &narrowed)], NEXTEST_SUMMARY);
    Ok(status == Some(0) && summary.as_deref().and_then(ran).is_some_and(|ran| usize::try_from(ran).is_ok_and(|ran| ran >= class.len())))
}

/// Keeps on the declared line every crate of `passed` that has a case in the class, read from the landing `base` and never from the lane at `root`, for a line that names its
/// own packages and tests: the class run narrows the line that reads the selection variables, which such a line does
/// not, so Buck2, which skips those cases, decides none of those crates there.
fn hold(root: &Path, base: Option<&str>, passed: &mut BTreeSet<String>) -> Result<bool, String> {
    let package = |binary: &str| binary.split_once("::").map_or(binary, |(package, _)| package).to_owned();
    let held: BTreeSet<String> = north_buck::host_class::reviewed(root, base)?.into_iter().map(|(binary, _)| package(&binary)).filter(|name| passed.remove(name)).collect();
    for name in &held {
        eprintln!("north land crate-tests: {name} has host cases this line cannot be narrowed to, so the declared line tests it");
    }
    Ok(true)
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
    let named = north_build_system::named_selection(declared, PACKAGES_ENV);
    let (line, selected) = match &named {
        Some((line, packages)) => (line.as_str(), Some(packages.clone())),
        None => (declared, variable(PACKAGES_ENV)),
    };
    let base = variable(BASE_ENV);
    let split = std::env::current_dir().ok().and_then(|root| north_build_system::crate_tests(&root, base.as_deref(), line, PACKAGES_ENV, selected.as_deref()));
    let Some(split) = split else { return declared_line(declared, None) };
    let held = match split.hold(Path::new(".")) {
        Ok(held) => held,
        Err(why) => {
            eprintln!("north land crate-tests: {why}, so the declared line tests every crate");
            return declared_line(declared, None);
        }
    };
    eprintln!("north land crate-tests: this host tests {} with Buck2: {}", split.moved.join(", "), split.line);
    let (status, output) = run_printing(&split.line, &[]);
    let summary = last_with(&output, SUMMARY);
    let record = north_build_system::test_record(&split.line).filter(|_| status == Some(0)).and_then(|record| record.runs(Path::new(".")).map_err(|why| eprintln!("north land crate-tests: cannot read Buck2's record of the run: {why}")).ok());
    drop(held);
    let owner = |label: &str| split.owner(label).map(str::to_owned);
    let error = remote_error(&output);
    let Verdict::Passed(mut passed) = verdict(status, summary.as_deref(), record.as_deref(), &owner, &split.caseless, error.is_some()) else { return FAILED };
    for name in split.moved.iter().filter(|name| !passed.contains(*name)) {
        let cases: u64 = record.iter().flatten().filter(|run| split.owner(&run.target) == Some(name.as_str())).filter_map(TestRun::cases).map(|cases| cases.run).sum();
        eprintln!("north land crate-tests: Buck2 gave no verdict on {name} (exit {}, {cases} case(s) of it in its record), so the declared line tests it", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string()));
    }
    let cases = if named.is_some() { hold(Path::new("."), base.as_deref(), &mut passed) } else { host_cases(Path::new("."), base.as_deref(), line, &passed, &split) };
    match cases {
        Ok(true) => {}
        Ok(false) => return FAILED,
        Err(why) => {
            eprintln!("north land crate-tests: cannot read the host cases ({why}), so the declared line tests every moved crate");
            passed.clear();
        }
    }
    eprintln!("{}", check_log::crates_split_line(passed.len(), split.declared_crates(&passed)));
    let rerun = split.left(&passed).map_or(0, |packages| declared_line(&split.left_line(line, &passed), Some(&packages)));
    if let Some(error) = error {
        let outcome = if rerun == 0 { "passed" } else { "failed" };
        let (head, targets) = (head(), failed_targets(&output).join(", "));
        eprintln!("north land crate-tests: infrastructure: head={head} target={targets} error={error} outcome={outcome} on the declared line, run once more");
        record_infrastructure(&infrastructure_record(&head, &targets, error, outcome, epoch_seconds()));
    }
    rerun
}

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// The `north ledger incident record` arguments for one infrastructure attempt: the head, the failed targets, the error
/// text and the outcome of the rerun, each attempt under its own key (north#1115).
fn infrastructure_record(head: &str, targets: &str, error: &str, outcome: &str, at: u64) -> Vec<String> {
    let detail = format!("head={head} target={targets} error={error} outcome={outcome}");
    ["ledger", "incident", "record", "--key", &format!("{INFRASTRUCTURE_CLASS}:{head}:{at}"), "--class", INFRASTRUCTURE_CLASS, "--noticed-by", "land-check", "--detail", &detail].map(str::to_owned).into()
}

/// Writes one infrastructure attempt through this north's `ledger incident record`; a failed write is printed and the check goes on.
fn record_infrastructure(arguments: &[String]) {
    let written = std::env::current_exe().map_err(|why| why.to_string()).and_then(|north| Command::new(north).args(arguments).stdin(Stdio::null()).status().map_err(|why| why.to_string()));
    if let Err(why) = written.and_then(|status| if status.success() { Ok(()) } else { Err(format!("exit {status}")) }) {
        eprintln!("north land crate-tests: could not record the infrastructure attempt: {why}");
    }
}

/// `north land crate-tests host-cases [PACKAGE...]`: the host cases of `packages`, every crate of the class when none, run
/// as a host with `buck2_host_cases` runs them in a check, whatever this host's policy says.
fn host_cases_here(packages: &[String]) -> u8 {
    let root = Path::new(".");
    let chosen = |binary: &String| packages.is_empty() || packages.iter().any(|package| package == binary.split_once("::").map_or(binary.as_str(), |(name, _)| name));
    let class = north_buck::host_class::reviewed(root, north_buck::host_class::landing_base().as_deref()).map(|class| class.into_iter().filter(|(binary, _)| chosen(binary)).collect::<Vec<_>>());
    match class.and_then(|class| if class.is_empty() { Err(format!("the class names no host case of {}", packages.join(", "))) } else { Ok(class) }).and_then(|class| built_host_cases(&class, |cases| north_build_system::host_case_runs(root, north_buck::host_class::TARGET, cases))) {
        Ok(true) => 0,
        Ok(false) => FAILED,
        Err(why) => {
            eprintln!("north land crate-tests host-cases: {why}");
            FAILED
        }
    }
}

/// `north land crate-tests after-landing`: Buck2's verdict on the landed tree in this checkout for every crate this host lists, with no Cargo line and no host case: prints each crate it passed on its own line after [`PASSED`]; exits 0 when it failed none, 1 when it failed the tree, and [`NO_VERDICT`] when it decided nothing.
fn after_landing() -> u8 {
    let split = std::env::current_dir().ok().and_then(|root| north_build_system::crate_tests(&root, Some("HEAD"), AFTER_LANDING_LINE, PACKAGES_ENV, None));
    let Some(split) = split else {
        eprintln!("north land crate-tests after-landing: this host lists no crate for Buck2, holds no store generation, or the tree has no Buck2 targets, so Buck2 gives no verdict");
        return NO_VERDICT;
    };
    let held = match split.hold(Path::new(".")) {
        Ok(held) => held,
        Err(why) => {
            eprintln!("north land crate-tests after-landing: {why}, so Buck2 gives no verdict");
            return NO_VERDICT;
        }
    };
    eprintln!("north land crate-tests after-landing: this host tests {} with Buck2: {}", split.moved.join(", "), split.line);
    let (status, output) = run_printing(&split.line, &[]);
    let summary = last_with(&output, SUMMARY);
    let record = north_build_system::test_record(&split.line).filter(|_| status == Some(0)).and_then(|record| record.runs(Path::new(".")).map_err(|why| eprintln!("north land crate-tests after-landing: cannot read Buck2's record of the run: {why}")).ok());
    drop(held);
    let owner = |label: &str| split.owner(label).map(str::to_owned);
    match verdict(status, summary.as_deref(), record.as_deref(), &owner, &split.caseless, remote_error(&output).is_some()) {
        Verdict::Failed => FAILED,
        Verdict::Passed(passed) if passed.is_empty() => NO_VERDICT,
        Verdict::Passed(passed) => {
            for name in &passed {
                println!("{PASSED}{name}");
            }
            eprintln!("{}", check_log::crates_split_line(passed.len(), 0));
            0
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb] if verb == "available" => 0,
        [verb] if verb == "after-run-available" => u8::from(!north_build_system::after_run_here()),
        [verb] if verb == "after-landing" => after_landing(),
        [separator, command] if separator == "--" => execute(command),
        [verb, packages @ ..] if verb == "host-cases" => host_cases_here(packages),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}

#[cfg(test)]
#[path = "listed_crates_tests.rs"]
mod tests;
