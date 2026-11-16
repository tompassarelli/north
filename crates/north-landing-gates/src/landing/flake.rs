use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use north_base::platform::process::child_within;
use super::env::{Stop, git, git_text};
use super::trees::EVERY_TEST;

/// The exit status of the build line when the tests do not build; any other status without a verdict stops the check.
pub const TESTS_DO_NOT_BUILD: u8 = 2;
const DEFAULT_RUNS: usize = 8;
const DEADLINE: Duration = Duration::from_secs(120);
const TAIL_LINES: usize = 30;

struct Setting {
    build: String,
    runs: usize,
}

struct Batch {
    exe: PathBuf,
    tests: Vec<String>,
}

struct Finished {
    code: Option<i32>,
    output: String,
}

pub enum Verdict {
    Pass,
    Flaky(usize),
    Unlisted,
}

pub enum Reported {
    Passed,
    Failed,
    Silent,
}

struct Case {
    name: String,
    path: String,
    batch: usize,
}

struct Ran {
    cases: Vec<Case>,
    finished: Vec<Vec<Finished>>,
}

impl Ran {
    fn wanted(&self, names: &BTreeSet<String>) -> BTreeSet<String> {
        if names.contains(EVERY_TEST) {
            self.cases.iter().map(|case| case.name.clone()).collect()
        } else {
            names.clone()
        }
    }

    fn runs_of(&self, name: &str) -> Vec<(&str, &Finished)> {
        self.cases
            .iter()
            .filter(|case| case.name == name)
            .flat_map(|case| self.finished[case.batch].iter().map(move |run| (case.path.as_str(), run)))
            .collect()
    }
}

fn setting(cwd: &Path, base: &str) -> Result<Option<Setting>, Stop> {
    let blob = format!("{base}:.safe-push");
    if !git(cwd, None, &["cat-file", "-e", &blob])?.status.success() {
        return Ok(None);
    }
    let get = |key: &str| -> Result<Option<String>, Stop> {
        let output = git(cwd, None, &["config", "--blob", &blob, "--get", key])?;
        match output.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())),
            Some(1) => Ok(None),
            _ => Err(Stop::Fail(format!("flake: cannot read {blob}: {}", String::from_utf8_lossy(&output.stderr).trim()))),
        }
    };
    let Some(build) = get("flake.build")?.filter(|build| !build.is_empty()) else {
        return Ok(None);
    };
    let runs = match get("flake.runs")? {
        Some(text) => text.parse().ok().filter(|runs| *runs >= 2).ok_or_else(|| Stop::Fail(format!("flake: flake.runs in {blob} must be a number of at least 2, not {text}")))?,
        None => DEFAULT_RUNS,
    };
    Ok(Some(Setting { build, runs }))
}

pub(super) fn check(cwd: &Path, base: &str, head: &str, names: &BTreeSet<String>, out: &mut dyn Write) -> Result<(), Stop> {
    let Some(setting) = setting(cwd, base)? else {
        writeln!(out, "fails-first: flake runs skipped ({base}:.safe-push sets no flake.build)")?;
        return Ok(());
    };
    let tree = |rev: &str| git_text(cwd, None, &["rev-parse", &format!("{rev}^{{tree}}")]);
    if tree("HEAD")? != tree(head)? {
        return unverified(out, "the checked-out tree is not the lane head");
    }
    let started = Instant::now();
    let Some(printed) = build(cwd, &setting.build, names, setting.runs)? else {
        return unverified(out, "the lane's tests do not build");
    };
    let verdicts = parse_verdicts(&printed)?;
    if let Some(name) = names.iter().find(|name| !verdicts.contains_key(name.as_str())) {
        return Err(Stop::Fail(format!("flake: the build line gave no verdict for {name}")));
    }
    let unlisted: Vec<&str> = verdicts.iter().filter(|(_, verdict)| matches!(verdict, Verdict::Unlisted)).map(|(name, _)| *name).collect();
    if !unlisted.is_empty() {
        writeln!(out, "safe-push: ✗ fails-first: no built test executable lists the added tests, so they cannot be run: {}; NOT pushed.", unlisted.join(", "))?;
        return Err(Stop::Refused);
    }
    let flaky: Vec<(&str, usize)> = verdicts.iter().filter_map(|(name, verdict)| if let Verdict::Flaky(count) = verdict { Some((*name, *count)) } else { None }).collect();
    if !flaky.is_empty() {
        for (test, count) in &flaky {
            writeln!(out, "fails-first: flaky {test} failed {count} of {} runs", setting.runs)?;
        }
        let listed: Vec<&str> = flaky.iter().map(|(test, _)| *test).collect();
        writeln!(out, "safe-push: ✗ fails-first: these tests fail when run {} times concurrently, so they are flaky: {}; make them deterministic, never retry them to green; NOT pushed.", setting.runs, listed.join(", "))?;
        return Err(Stop::Refused);
    }
    writeln!(out, "fails-first: ✓ {} changed test(s) passed {} concurrent runs ({:.1} s)", names.len(), setting.runs, started.elapsed().as_secs_f64())?;
    Ok(())
}

fn parse_verdicts(printed: &str) -> Result<BTreeMap<&str, Verdict>, Stop> {
    let mut verdicts = BTreeMap::new();
    for line in printed.lines() {
        let verdict = match line.split(' ').collect::<Vec<_>>().as_slice() {
            ["pass", name] => (*name, Verdict::Pass),
            ["unlisted", name] => (*name, Verdict::Unlisted),
            ["flaky", name, count] => (*name, Verdict::Flaky(count.parse().map_err(|_| Stop::Fail(format!("flake: no count in the verdict line {line}")))?)),
            _ => return Err(Stop::Fail(format!("flake: the build line printed an unknown verdict line: {line}"))),
        };
        verdicts.insert(verdict.0, verdict.1);
    }
    Ok(verdicts)
}

fn unverified(out: &mut dyn Write, reason: &str) -> Result<(), Stop> {
    writeln!(out, "fails-first: flake runs unverified ({reason})")?;
    Ok(())
}

pub fn executables(cwd: &Path, log: &mut dyn Write) -> Result<Vec<PathBuf>, Option<String>> {
    let side = north_build_system::here();
    let invocation = side.test_build(cwd).map_err(Some)?;
    let _lock = side.test_lock(cwd).map_err(Some)?;
    let output = invocation.command().stdin(Stdio::null()).output().map_err(|error| Some(error.to_string()))?;
    let text = String::from_utf8_lossy(&output.stderr).into_owned();
    log.write_all(text.as_bytes()).map_err(|error| Some(error.to_string()))?;
    if !output.status.success() {
        return Err(if side.broken(output.status.code(), &text) { None } else { Some(format!("the build exited {}", output.status)) });
    }
    let found = side.test_executables(cwd, &String::from_utf8_lossy(&output.stdout)).map_err(Some)?;
    Ok(found.iter().map(|executable| cwd.join(executable)).collect())
}

pub fn verdicts(cwd: &Path, executables: &[PathBuf], names: &BTreeSet<String>, runs: usize, log: &mut dyn Write) -> io::Result<BTreeMap<String, Verdict>> {
    let ran = run_named(cwd, executables, names, runs);
    let wanted = ran.wanted(names);
    let known = wanted.iter().filter(|name| !ran.runs_of(name).is_empty()).count();
    writeln!(log, "fails-first: running {known} changed test(s) {runs} times concurrently on the lane head")?;
    let mut judged = BTreeMap::new();
    for name in wanted {
        let found = ran.runs_of(&name);
        if found.is_empty() {
            judged.insert(name, Verdict::Unlisted);
            continue;
        }
        let failures: Vec<(&str, &Finished)> = found.iter().copied().filter(|(path, run)| failed(run, path)).collect();
        let verdict = match failures.first() {
            None => Verdict::Pass,
            Some((_, first)) => {
                let lines: Vec<&str> = first.output.lines().collect();
                writeln!(log, "fails-first: output of a failing {name} run ends:\n{}", lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n"))?;
                Verdict::Flaky(failures.len())
            }
        };
        judged.insert(name, verdict);
    }
    Ok(judged)
}

/// The named tests' results for the kill check: a test is failed only when its own line says FAILED in some run, and it is recorded only when its own line says ok in every run; a batch that dies or exits without that line leaves the test unrecorded.
pub fn reported(cwd: &Path, executables: &[PathBuf], names: &BTreeSet<String>, runs: usize) -> BTreeMap<String, Reported> {
    let ran = run_named(cwd, executables, names, runs);
    let mut reported = BTreeMap::new();
    for name in ran.wanted(names) {
        let readings: Vec<Option<bool>> = ran.runs_of(&name).into_iter().map(|(path, run)| outcome(run, path)).collect();
        if readings.is_empty() {
            continue;
        }
        let verdict = if readings.contains(&Some(false)) {
            Reported::Failed
        } else if readings.iter().all(|reading| *reading == Some(true)) {
            Reported::Passed
        } else {
            Reported::Silent
        };
        reported.insert(name, verdict);
    }
    reported
}

fn run_named(cwd: &Path, executables: &[PathBuf], names: &BTreeSet<String>, runs: usize) -> Ran {
    let batches = batches(executables, names);
    let cases = batches
        .iter()
        .enumerate()
        .flat_map(|(index, batch)| batch.tests.iter().map(move |path| Case { name: name_of(path).to_owned(), path: path.clone(), batch: index }))
        .collect();
    Ran { cases, finished: run(cwd, &batches, runs) }
}

pub fn print(verdicts: &BTreeMap<String, Verdict>, out: &mut dyn Write) -> io::Result<()> {
    for (name, verdict) in verdicts {
        match verdict {
            Verdict::Pass => writeln!(out, "pass {name}")?,
            Verdict::Flaky(count) => writeln!(out, "flaky {name} {count}")?,
            Verdict::Unlisted => writeln!(out, "unlisted {name}")?,
        }
    }
    Ok(())
}

fn name_of(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn selected(names: &BTreeSet<String>, test: &str) -> bool {
    names.contains(EVERY_TEST) || names.contains(test)
}

fn own_line<'a>(run: &'a Finished, test: &str) -> Option<&'a str> {
    run.output.lines().find_map(|line| {
        let (name, verdict) = line.strip_prefix("test ")?.rsplit_once(" ... ")?;
        (name.strip_suffix(" - should panic").unwrap_or(name) == test).then_some(verdict)
    })
}

fn failed(run: &Finished, test: &str) -> bool {
    let seen = own_line(run, test).map(|verdict| verdict.starts_with("ok"));
    seen == Some(false) || (run.code != Some(0) && seen != Some(true))
}

fn outcome(run: &Finished, test: &str) -> Option<bool> {
    match own_line(run, test)? {
        "ok" => Some(true),
        "FAILED" => Some(false),
        _ => None,
    }
}

fn build(cwd: &Path, command: &str, names: &BTreeSet<String>, runs: usize) -> Result<Option<String>, Stop> {
    let added: Vec<&str> = names.iter().map(String::as_str).collect();
    let line = format!("{command} {runs} {}", added.join(" "));
    let output = Command::new("sh").args(["-c", &line]).current_dir(cwd).stdin(Stdio::null()).stderr(Stdio::inherit()).output()?;
    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
        Some(code) if code == i32::from(TESTS_DO_NOT_BUILD) => Ok(None),
        code => Err(Stop::Fail(format!("the build line {command} gave no verdict (exit {})", code.map_or_else(|| "none".to_owned(), |code| code.to_string())))),
    }
}

fn batches(executables: &[PathBuf], names: &BTreeSet<String>) -> Vec<Batch> {
    executables
        .iter()
        .filter_map(|exe| {
            let listing = Command::new(exe).args(["--list", "--format", "terse"]).stdin(Stdio::null()).output().ok()?;
            let text = String::from_utf8_lossy(&listing.stdout);
            let tests: Vec<String> = text
                .lines()
                .filter_map(|line| line.strip_suffix(": test"))
                .filter(|path| selected(names, name_of(path)))
                .map(str::to_owned)
                .collect();
            (!tests.is_empty()).then(|| Batch { exe: exe.clone(), tests })
        })
        .collect()
}

fn run(cwd: &Path, batches: &[Batch], runs: usize) -> Vec<Vec<Finished>> {
    let idle = AtomicBool::new(false);
    let load = if runs > 1 { 2 * thread::available_parallelism().map_or(1, usize::from) } else { 0 };
    thread::scope(|scope| {
        for _ in 0..load {
            scope.spawn(|| {
                while !idle.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            });
        }
        let handles: Vec<Vec<_>> = batches
            .iter()
            .map(|batch| (0..runs).map(|_| scope.spawn(move || execute(cwd, batch))).collect())
            .collect();
        let finished = handles.into_iter().map(|row| row.into_iter().map(|handle| handle.join().unwrap_or_else(|_| lost())).collect()).collect();
        idle.store(true, Ordering::Relaxed);
        finished
    })
}

fn lost() -> Finished {
    Finished { code: None, output: "the run thread panicked\n".to_owned() }
}

fn execute(cwd: &Path, batch: &Batch) -> Finished {
    let outcome = (|| -> io::Result<Finished> {
        let mut log = tempfile::tempfile()?;
        let mut child = spawn(cwd, batch, &log)?;
        let code = child_within(&mut child, DEADLINE)?.and_then(|status| status.code());
        let mut bytes = Vec::new();
        log.rewind()?;
        log.read_to_end(&mut bytes)?;
        Ok(Finished { code, output: String::from_utf8_lossy(&bytes).into_owned() })
    })();
    outcome.unwrap_or_else(|error| Finished { code: Some(127), output: format!("could not run {}: {error}\n", batch.exe.display()) })
}

fn spawn(cwd: &Path, batch: &Batch, log: &File) -> io::Result<Child> {
    let mut busy = 0;
    loop {
        let mut command = Command::new(&batch.exe);
        command.current_dir(cwd).arg("--exact").args(&batch.tests).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log.try_clone()?);
        match command.spawn() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && busy < 50 => {
                busy += 1;
                thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
}
