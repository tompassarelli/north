use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use north_base::platform::process::child_within;
use north_mutants::env::{Stop, git, git_text};

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

enum Verdict {
    Pass,
    Flaky(usize),
    Unlisted,
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

/// The verdict line of each added test the build line runs: `pass NAME`, `flaky NAME FAILED_RUNS`, or `unlisted NAME` when no built executable lists it. Progress and the output of a failing run go to `log`.
pub fn verdicts(cwd: &Path, executables: &[PathBuf], names: &BTreeSet<String>, runs: usize, log: &mut dyn Write, out: &mut dyn Write) -> io::Result<()> {
    let batches = batches(executables, names);
    let cases: Vec<(&str, usize, &str)> = batches.iter().enumerate().flat_map(|(index, batch)| batch.tests.iter().map(move |path| (name_of(path), index, path.as_str()))).collect();
    let known = names.iter().filter(|name| cases.iter().any(|(case, ..)| case == name)).count();
    writeln!(log, "fails-first: running {known} changed test(s) {runs} times concurrently on the lane head")?;
    let finished = run(cwd, &batches, runs);
    for name in names {
        let found: Vec<(usize, &str)> = cases.iter().filter(|(case, ..)| case == name).map(|(_, index, path)| (*index, *path)).collect();
        if found.is_empty() {
            writeln!(out, "unlisted {name}")?;
            continue;
        }
        let failures: Vec<&Finished> = found.iter().flat_map(|(index, path)| finished[*index].iter().filter(|run| failed(run, path))).collect();
        match failures.first() {
            None => writeln!(out, "pass {name}")?,
            Some(first) => {
                let lines: Vec<&str> = first.output.lines().collect();
                writeln!(log, "fails-first: output of a failing {name} run ends:\n{}", lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n"))?;
                writeln!(out, "flaky {name} {}", failures.len())?;
            }
        }
    }
    Ok(())
}

fn name_of(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn failed(run: &Finished, test: &str) -> bool {
    let verdict = |line: &str| -> Option<bool> {
        let (name, verdict) = line.strip_prefix("test ")?.rsplit_once(" ... ")?;
        (name == test).then(|| verdict.starts_with("ok"))
    };
    let seen = run.output.lines().filter_map(verdict).next();
    seen == Some(false) || (run.code != Some(0) && seen != Some(true))
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
                .filter(|path| names.contains(name_of(path)))
                .map(str::to_owned)
                .collect();
            (!tests.is_empty()).then(|| Batch { exe: exe.clone(), tests })
        })
        .collect()
}

fn run(cwd: &Path, batches: &[Batch], runs: usize) -> Vec<Vec<Finished>> {
    let idle = AtomicBool::new(false);
    let load = 2 * thread::available_parallelism().map_or(1, usize::from);
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
