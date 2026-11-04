use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::mutant::{Stop, git, git_text};

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
    let Some(executables) = build(cwd, &setting.build)? else {
        return unverified(out, "the lane's tests do not build");
    };
    let batches = batches(&executables, names);
    let tests: usize = batches.iter().map(|batch| batch.tests.len()).sum();
    if tests == 0 {
        return unverified(out, "no built test executable lists a changed test");
    }
    writeln!(out, "fails-first: running {tests} changed test(s) {} times concurrently on the lane head", setting.runs)?;
    let finished = run(cwd, &batches, setting.runs);
    let mut flaky = BTreeMap::<&str, (usize, &Finished)>::new();
    for (batch, runs) in batches.iter().zip(&finished) {
        for test in &batch.tests {
            let failures: Vec<&Finished> = runs.iter().filter(|run| failed(run, test)).collect();
            if let Some(first) = failures.first() {
                flaky.insert(test, (failures.len(), first));
            }
        }
    }
    if let Some((test, (_, run))) = flaky.iter().next() {
        for (test, (count, _)) in &flaky {
            writeln!(out, "fails-first: flaky {test} failed {count} of {} runs", setting.runs)?;
        }
        let lines: Vec<&str> = run.output.lines().collect();
        writeln!(out, "fails-first: output of a failing {test} run ends:\n{}", lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n"))?;
        let listed: Vec<&str> = flaky.keys().copied().collect();
        writeln!(out, "safe-push: ✗ fails-first: these tests fail when run {} times concurrently, so they are flaky: {}; make them deterministic, never retry them to green; NOT pushed.", setting.runs, listed.join(", "))?;
        return Err(Stop::Refused);
    }
    writeln!(out, "fails-first: ✓ {tests} changed test(s) passed {} concurrent runs ({:.1} s)", setting.runs, started.elapsed().as_secs_f64())?;
    Ok(())
}

fn unverified(out: &mut dyn Write, reason: &str) -> Result<(), Stop> {
    writeln!(out, "fails-first: flake runs unverified ({reason})")?;
    Ok(())
}

fn failed(run: &Finished, test: &str) -> bool {
    let verdict = |line: &str| -> Option<bool> {
        let (name, verdict) = line.strip_prefix("test ")?.rsplit_once(" ... ")?;
        (name == test).then(|| verdict.starts_with("ok"))
    };
    let seen = run.output.lines().filter_map(verdict).next();
    seen == Some(false) || (run.code != Some(0) && seen != Some(true))
}

fn build(cwd: &Path, command: &str) -> Result<Option<Vec<PathBuf>>, Stop> {
    let output = Command::new("sh").args(["-c", command]).current_dir(cwd).stdin(Stdio::null()).stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let mut found = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let executable = (message["reason"] == "compiler-artifact" && message["profile"]["test"] == true).then(|| message["executable"].as_str()).flatten();
        if let Some(path) = executable.map(PathBuf::from).filter(|path| !found.contains(path)) {
            found.push(path);
        }
    }
    Ok(Some(found))
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
                .filter(|path| names.contains(path.rsplit("::").next().unwrap_or(path)))
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
        let started = Instant::now();
        let code = loop {
            if let Some(status) = child.try_wait()? {
                break status.code();
            }
            if started.elapsed() > DEADLINE {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
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
