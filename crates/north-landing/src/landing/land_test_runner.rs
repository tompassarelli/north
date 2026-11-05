use super::base_relative::Candidate;
use super::land_test_selection::{self, ChildEnv, Only, Scope};
pub use super::results::Case;
use std::io::{self, Read, Seek, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const USAGE: &str = "usage: north land-tests [--jobs N] [--cases json] TEST...\n       north land-tests [--jobs N] [--cases json] --select changed|all [--only portable|machine-bound]";

struct Outcome {
    test: String,
    status: Option<i32>,
    seconds: u64,
    output: Vec<u8>,
}

enum Event<'a> {
    Started(&'a str),
    Finished(Outcome),
}

impl Outcome {
    fn passed(&self) -> bool {
        self.status == Some(0)
    }
}

enum Tests<'a> {
    Named(&'a [String]),
    Selected(Scope, Only),
}

struct Request<'a> {
    jobs: NonZeroUsize,
    cases: bool,
    tests: Tests<'a>,
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(request) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let (selected, env) = match request.tests {
        Tests::Named(tests) => (tests.to_vec(), ChildEnv::default()),
        Tests::Selected(scope, only) => {
            let env = ChildEnv::for_selection(Path::new("."));
            match land_test_selection::select(Path::new("."), scope, only, &env) {
                Ok(selected) if selected.is_empty() => return 0,
                Ok(selected) => {
                    let north = std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_else(|_| "north".to_owned());
                    println!("land check: running {north} land-tests {}", selected.join(" "));
                    (selected, env)
                }
                Err(failure) => {
                    eprintln!("{}", failure.message);
                    return failure.status;
                }
            }
        }
    };
    let tests = selected.as_slice();
    let candidate = Candidate::from_env();
    let outcomes = if request.cases {
        let outcomes = run_all(tests, request.jobs, &env, &mut io::stderr().lock());
        let mut stdout = io::stdout().lock();
        for case in tests.iter().flat_map(|test| outcomes.iter().filter(move |outcome| outcome.test == *test)).flat_map(cases) {
            let _ = writeln!(stdout, "{}", case.row());
        }
        outcomes
    } else {
        run_all(tests, request.jobs, &env, &mut io::stdout().lock())
    };
    let failed: Vec<&str> = outcomes.iter().filter(|outcome| !outcome.passed()).map(|outcome| outcome.test.as_str()).collect();
    if !failed.is_empty() {
        eprintln!("land check: {} of {} tests failed: {}", failed.len(), tests.len(), failed.join(", "));
    }
    match candidate {
        Some(candidate) => candidate.judge(&outcomes.iter().flat_map(cases).collect::<Vec<_>>(), &mut io::stderr().lock()),
        None => u8::from(!failed.is_empty()),
    }
}

fn parse(arguments: &[String]) -> Option<Request<'_>> {
    let mut jobs = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let mut cases = false;
    let mut scope = None;
    let mut only = Only::Both;
    let mut rest = arguments;
    loop {
        match rest {
            [flag, value, ..] if flag == "--jobs" => jobs = value.parse().ok()?,
            [flag, value, ..] if flag == "--cases" && value == "json" => cases = true,
            [flag, value, ..] if flag == "--select" && value == "changed" => scope = Some(Scope::Changed),
            [flag, value, ..] if flag == "--select" && value == "all" => scope = Some(Scope::All),
            [flag, value, ..] if flag == "--only" && value == "portable" => only = Only::Portable,
            [flag, value, ..] if flag == "--only" && value == "machine-bound" => only = Only::MachineBound,
            [flag, ..] if ["--jobs", "--cases", "--select", "--only"].contains(&flag.as_str()) => return None,
            _ => break,
        }
        rest = &rest[2..];
    }
    let tests = match scope {
        Some(scope) if rest.is_empty() => Tests::Selected(scope, only),
        None if !rest.is_empty() && only == Only::Both => Tests::Named(rest),
        _ => return None,
    };
    Some(Request { jobs, cases, tests })
}

fn run_all(tests: &[String], jobs: NonZeroUsize, env: &ChildEnv, out: &mut impl Write) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut outcomes = Vec::new();
    thread::scope(|scope| {
        for _ in 0..jobs.get().min(tests.len()) {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                while let Some(test) = tests.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if sender.send(Event::Started(test)).is_err() || sender.send(Event::Finished(run_one(test, env))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        for event in receiver {
            match event {
                Event::Started(test) => {
                    let _ = writeln!(out, "land check: started {test}");
                    let _ = out.flush();
                }
                Event::Finished(outcome) => {
                    report(&outcome, out);
                    outcomes.push(outcome);
                }
            }
        }
    });
    outcomes
}

fn case_line(line: &str) -> Option<(String, bool)> {
    if let Some(rest) = line.strip_prefix("test ") {
        let (name, verdict) = rest.rsplit_once(" ... ")?;
        return match verdict.split_whitespace().next()? {
            "ok" => Some((name.trim().to_owned(), true)),
            "FAILED" => Some((name.trim().to_owned(), false)),
            _ => None,
        };
    }
    for (prefix, passed) in [("PASS", true), ("FAIL", false)] {
        let name = line.strip_prefix(prefix).and_then(|rest| rest.strip_prefix(": ").or_else(|| rest.strip_prefix(' ')));
        if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
            return Some((name.to_owned(), passed));
        }
    }
    None
}

fn cases(outcome: &Outcome) -> Vec<Case> {
    let text = String::from_utf8_lossy(&outcome.output);
    let mut found: Vec<Case> = text
        .lines()
        .filter_map(case_line)
        .map(|(name, passed)| Case { test: outcome.test.clone(), name, passed })
        .collect();
    let hidden_failure = !outcome.passed() && found.iter().all(|case| case.passed);
    if found.is_empty() || hidden_failure {
        found.push(Case { test: outcome.test.clone(), name: outcome.test.clone(), passed: outcome.passed() });
    }
    found
}

fn run_one(test: &str, env: &ChildEnv) -> Outcome {
    let started = Instant::now();
    let (status, output) = match execute(test, env) {
        Ok(result) => result,
        Err(error) => (Some(127), format!("land check: could not run {test}: {error}\n").into_bytes()),
    };
    Outcome { test: test.to_owned(), status, seconds: started.elapsed().as_secs(), output }
}

fn execute(test: &str, env: &ChildEnv) -> io::Result<(Option<i32>, Vec<u8>)> {
    let mut command = match Path::new(test).extension().and_then(|extension| extension.to_str()) {
        Some("py") => interpreted("python3", test),
        Some("sh") => interpreted("bash", test),
        None => Command::new(test),
        Some(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "a land-check test is a .sh or .py file or a built test executable")),
    };
    let mut log = tempfile::tempfile()?;
    env.apply(&mut command).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log.try_clone()?);
    let mut busy = 0;
    let status = loop {
        match command.status() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && busy < 50 => {
                busy += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => break result?,
        }
    };
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
