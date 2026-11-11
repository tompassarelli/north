use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output};
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

fn one(binary: &Path, name: &str) -> Result<Outcome, String> {
    let started = Instant::now();
    let Output { status, stdout, stderr } =
        Command::new(binary).args(["--exact", name, "--nocapture"]).output().map_err(|error| format!("cannot run {} {name}: {error}", binary.display()))?;
    let verdict = match status.signal() {
        Some(signal) => Verdict::Signal(signal),
        None if status.success() => Verdict::Pass,
        None => Verdict::Fail,
    };
    Ok(Outcome { name: name.to_owned(), verdict, took: started.elapsed(), output: [stdout, stderr].concat() })
}
