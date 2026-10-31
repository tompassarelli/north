use std::io::{self, Read, Seek, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const USAGE: &str = "usage: north land-tests [--jobs N] TEST...";

struct Outcome {
    test: String,
    status: Option<i32>,
    seconds: u64,
    output: Vec<u8>,
}

impl Outcome {
    fn passed(&self) -> bool {
        self.status == Some(0)
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((jobs, tests)) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let failed = run_all(tests, jobs, &mut io::stdout().lock());
    if failed.is_empty() {
        return 0;
    }
    eprintln!("land check: {} of {} tests failed: {}", failed.len(), tests.len(), failed.join(", "));
    1
}

fn parse(arguments: &[String]) -> Option<(NonZeroUsize, &[String])> {
    let (jobs, tests) = match arguments {
        [flag, jobs, tests @ ..] if flag == "--jobs" => (jobs.parse().ok()?, tests),
        [flag, ..] if flag == "--jobs" => return None,
        tests => (thread::available_parallelism().unwrap_or(NonZeroUsize::MIN), tests),
    };
    (!tests.is_empty()).then_some((jobs, tests))
}

fn run_all(tests: &[String], jobs: NonZeroUsize, out: &mut impl Write) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let (sender, outcomes) = mpsc::channel();
    let mut failed = Vec::new();
    thread::scope(|scope| {
        for _ in 0..jobs.get().min(tests.len()) {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                while let Some(test) = tests.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if sender.send(run_one(test)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        for outcome in outcomes {
            report(&outcome, out);
            if !outcome.passed() {
                failed.push(outcome.test);
            }
        }
    });
    failed
}

fn run_one(test: &str) -> Outcome {
    let started = Instant::now();
    let (status, output) = match execute(test) {
        Ok(result) => result,
        Err(error) => (Some(127), format!("land check: could not run {test}: {error}\n").into_bytes()),
    };
    Outcome { test: test.to_owned(), status, seconds: started.elapsed().as_secs(), output }
}

fn execute(test: &str) -> io::Result<(Option<i32>, Vec<u8>)> {
    let mut command = match Path::new(test).extension().and_then(|extension| extension.to_str()) {
        Some("py") => interpreted("python3", test),
        Some("sh") => interpreted("bash", test),
        None => Command::new(test),
        Some(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "a land-check test is a .sh or .py file or a built test executable")),
    };
    let mut log = tempfile::tempfile()?;
    let status = command
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .status()?;
    let mut output = Vec::new();
    log.rewind()?;
    log.read_to_end(&mut output)?;
    Ok((status.code(), output))
}

fn interpreted(interpreter: &str, test: &str) -> Command {
    let mut command = Command::new(interpreter);
    command.arg(test);
    command
}

fn report(outcome: &Outcome, out: &mut impl Write) {
    let _ = writeln!(out, "land check: {}", outcome.test);
    let _ = out.write_all(&outcome.output);
    if !outcome.output.is_empty() && !outcome.output.ends_with(b"\n") {
        let _ = writeln!(out);
    }
    let verdict = match outcome.status {
        Some(0) => "passed".to_owned(),
        Some(code) => format!("exited {code}"),
        None => "was killed by a signal".to_owned(),
    };
    let _ = writeln!(out, "land check: {} {verdict} after {} s", outcome.test, outcome.seconds);
    let _ = out.flush();
}

#[cfg(test)]
#[path = "land_test_runner_tests.rs"]
mod tests;
