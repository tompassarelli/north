use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub enum Verdict {
    Pass,
    Fail,
    Signal(i32),
}

pub struct Outcome {
    pub name: String,
    pub verdict: Verdict,
    pub took: Duration,
    pub output: Vec<u8>,
}

pub fn all(binary: &Path, names: &[String], threads: usize) -> Result<Vec<Outcome>, String> {
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::with_capacity(names.len()));
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(names.len()) {
            scope.spawn(|| {
                while let Some(name) = names.get(next.fetch_add(1, Ordering::Relaxed)) {
                    match one(binary, name) {
                        Ok(outcome) => outcomes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(outcome),
                        Err(error) => _ = failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(error),
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return Err(error);
    }
    let mut outcomes = outcomes.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    outcomes.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(outcomes)
}

/// A case writes to a file, never a pipe: the case's verdict is its own exit, so a process it leaves behind holding its output cannot keep the runner waiting, and what the case wrote is read whole however busy the host is.
fn one(binary: &Path, name: &str) -> Result<Outcome, String> {
    let (written, mut read) = capture().map_err(|error| format!("cannot capture the output of {name}: {error}"))?;
    let both = written.try_clone().map_err(|error| format!("cannot capture the output of {name}: {error}"))?;
    let started = Instant::now();
    let status = Command::new(binary)
        .args(["--exact", name, "--nocapture"])
        .stdin(Stdio::null())
        .stdout(written)
        .stderr(both)
        .status()
        .map_err(|error| format!("cannot run {} {name}: {error}", binary.display()))?;
    let took = started.elapsed();
    let mut output = Vec::new();
    read.read_to_end(&mut output).map_err(|error| format!("cannot read the output of {name}: {error}"))?;
    let verdict = match status.signal() {
        Some(signal) => Verdict::Signal(signal),
        None if status.success() => Verdict::Pass,
        None => Verdict::Fail,
    };
    Ok(Outcome { name: name.to_owned(), verdict, took, output })
}

/// An unnamed file in the temporary directory, as the handle a case appends to and the handle that reads it back.
fn capture() -> std::io::Result<(File, File)> {
    static CAPTURED: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("north-test-cases-{}-{}", std::process::id(), CAPTURED.fetch_add(1, Ordering::Relaxed)));
    let written = OpenOptions::new().append(true).create_new(true).open(&path)?;
    let read = File::open(&path);
    std::fs::remove_file(&path)?;
    Ok((written, read?))
}
