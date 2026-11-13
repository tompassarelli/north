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
//! checks before a landing alone wrap their line in this, and key the verdict apart, so the run after a landing tests the
//! landed tree with Cargo itself.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use north_build_system::{Cases, CrateTests, TestRun};

use super::affected::PACKAGES_ENV;
use super::it_modules::IT_ENV;
use super::unit_reuse::quote;

const USAGE: &str = "north land crate-tests available | north land crate-tests host-cases [PACKAGE...] | north land crate-tests -- COMMAND";
const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
/// Buck2's exit statuses for a build the tree's own targets or code failed, and for a test run that did not pass.
const BUILD_FAILED: i32 = 3;
const TESTS_FAILED: i32 = 32;
/// The line `buck2 test` ends on: `Tests finished: Pass 1. Fail 0. Timeout 0. ...`, each count possibly coloured.
const SUMMARY: &str = "Tests finished:";
/// The line cargo-nextest ends a run on: `Summary [ T s] N tests run: P passed, ...`, `1 test run` for one case.
const NEXTEST_SUMMARY: &str = "Summary [";
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

/// Whether the verdict of `command`, run before a landing with `selected` packages, or the packages it names itself,
/// may be Buck2's for part of it.
pub fn decides(command: &str, selected: Option<&str>) -> bool {
    let named = north_build_system::named_selection(command, PACKAGES_ENV).map(|(_, named)| named);
    wraps(command) && north_build_system::buck2_may_test(named.as_deref().or(selected))
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

/// What Buck2 said of the crates of one run, from its exit status, its summary line and its own record of the run, whose targets `owner` assigns to the crates. The record counts only when it holds a run for each target the summary counts as passed.
fn verdict(status: Option<i32>, summary: Option<&str>, runs: Option<&[TestRun]>, owner: &dyn Fn(&str) -> Option<String>, caseless: &BTreeSet<String>) -> Verdict {
    let counts = |label| summary.and_then(|summary| counted(summary, label));
    let recorded = runs.filter(|runs| u64::try_from(runs.len()).ok() == counts("Pass"));
    match (status, counts("Fail"), recorded) {
        (Some(0), Some(0), Some(runs)) => Verdict::Passed(passed(runs, owner, caseless)),
        (Some(BUILD_FAILED), ..) => Verdict::Failed,
        (Some(TESTS_FAILED), Some(failed), _) if failed > 0 => Verdict::Failed,
        _ => Verdict::Passed(BTreeSet::new()),
    }
}

/// Runs `line` with `environment` set and its output passed through, and returns its exit status and the last line it printed holding `summary`.
fn observed(line: &str, environment: &[(&str, &str)], summary: &str) -> (Option<i32>, Option<String>) {
    let mut command = Command::new("bash");
    command.args(["-euo", "pipefail", "-c", &format!("{{ {line}\n}} 2>&1")]).envs(environment.iter().copied()).stdout(Stdio::piped());
    let Ok(mut child) = command.spawn() else { return (None, None) };
    let mut last = None;
    for printed in child.stdout.take().map(BufReader::new).into_iter().flat_map(BufRead::lines).map_while(Result::ok) {
        eprintln!("{printed}");
        if printed.contains(summary) {
            last = Some(printed);
        }
    }
    (child.wait().ok().and_then(|status| status.code()), last)
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
    eprintln!("north land crate-tests: this host tests {} with Buck2: {}", split.moved.join(", "), split.line);
    let (status, summary) = observed(&split.line, &[], SUMMARY);
    let record = north_build_system::test_record(&split.line).filter(|_| status == Some(0)).and_then(|record| record.runs(Path::new(".")).map_err(|why| eprintln!("north land crate-tests: cannot read Buck2's record of the run: {why}")).ok());
    let owner = |label: &str| split.owner(label).map(str::to_owned);
    let Verdict::Passed(mut passed) = verdict(status, summary.as_deref(), record.as_deref(), &owner, &split.caseless) else { return FAILED };
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
    split.left(&passed).map_or(0, |packages| declared_line(line, Some(&packages)))
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

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb] if verb == "available" => 0,
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
