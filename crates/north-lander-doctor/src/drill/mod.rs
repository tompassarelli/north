//! `north land drill`: the nightly fault-injection drill (docs/rationale/lander-drill.md). It builds a scratch lander for each stuck state
//! the lander met on 2026-11-06, injects that state, and requires the remedy that owns it to clear it within the doctor's patience with
//! nobody paged. The drill runs in a child process whose XDG_STATE_HOME is the scratch, so nothing it calls writes the real lander's state.
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::book::Book;
use crate::bounds::{Bound, PATIENCE_SECS, Remedy};
use crate::facts::{self, Facts};
use crate::hands::{self, Hands};
use crate::{Treated, cure, render};

pub mod faults;
pub mod scratch;

use scratch::Lander;

const USAGE: &str = "usage: north land drill [FAULT]... (injects each lander stuck state into a scratch lander and requires its remedy to clear it within the doctor's patience, nobody paged; FAULT names one of: held-link, debt-gate, full-disk, wedged-unit, stale-runtime, dead-slot, flaky-red-main)";
const SCRATCH: &str = "--scratch";
const POLL: Duration = Duration::from_secs(2);
const INCIDENTS: &str = "incidents.log";
const PAGES: &str = "pages.log";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let (root, wanted) = match arguments {
        [flag, root, wanted @ ..] if flag == SCRATCH => (Some(Path::new(root)), wanted),
        wanted => (None, wanted),
    };
    if let Some(unknown) = wanted.iter().find(|name| !faults::FAULTS.iter().any(|fault| fault.key == name.as_str())) {
        eprintln!("north land drill: no fault named {unknown}\n{USAGE}");
        return 64;
    }
    match root {
        Some(root) => drill(root, wanted),
        None => scoped(wanted),
    }
}

/// Runs the drill in a child whose state home is a fresh scratch directory, and removes that directory when the child ends.
fn scoped(wanted: &[String]) -> u8 {
    let root = match tempfile::Builder::new().prefix("north-drill-").tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("north land drill: scratch directory: {error}");
            return 1;
        }
    };
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            eprintln!("north land drill: {error}");
            return 1;
        }
    };
    let status = Command::new(program).args(["land", "drill", SCRATCH]).arg(root.path()).args(wanted).env("XDG_STATE_HOME", root.path().join("drill-state")).stdin(Stdio::null()).status();
    status.ok().and_then(|status| status.code()).and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}

fn drill(root: &Path, wanted: &[String]) -> u8 {
    let started = Instant::now();
    let mut failed = 0;
    let chosen: Vec<&faults::Fault> = faults::FAULTS.iter().filter(|fault| wanted.is_empty() || wanted.iter().any(|name| name == fault.key)).collect();
    for (index, fault) in chosen.iter().enumerate() {
        let began = Instant::now();
        let result = Lander::open(&root.join(format!("{index}{}", fault.key.replace('-', "")))).and_then(|mut lander| (fault.drill)(&mut lander));
        match result {
            Ok(healed) => println!("north land drill: {}: healed in {:.1} s: {healed}", fault.name, began.elapsed().as_secs_f64()),
            Err(error) => {
                failed += 1;
                println!("north land drill: {}: NOT HEALED after {:.1} s: {error}", fault.name, began.elapsed().as_secs_f64());
            }
        }
    }
    println!("north land drill: {} of {} stuck states healed with nobody paged in {:.0} s", chosen.len() - failed, chosen.len(), started.elapsed().as_secs_f64());
    u8::from(failed > 0)
}

/// The doctor's hands on a scratch lander: every remedy runs as on the lander host, except that a promotion replays the scratch
/// repository, and incident rows and pages go to files in the scratch, where a page fails the drill.
struct Drilled<'a> {
    lander: &'a Lander,
}

impl Drilled<'_> {
    fn note(&self, file: &str, line: &str) -> bool {
        fs::OpenOptions::new().create(true).append(true).open(self.lander.root.join(file)).and_then(|mut log| std::io::Write::write_all(&mut log, format!("{}\n", line.replace('\n', " ")).as_bytes())).is_ok()
    }

    fn pages(&self) -> String {
        fs::read_to_string(self.lander.root.join(PAGES)).unwrap_or_default()
    }
}

impl Hands for Drilled<'_> {
    fn apply(&self, remedy: &Remedy) -> Result<String, String> {
        match remedy {
            Remedy::Promote => faults::promote(self.lander),
            Remedy::Alarm(text) => Err(format!("an alarm asks a person to act: {text}")),
            _ => hands::Host.apply(remedy),
        }
    }

    fn incident(&self, class: &str, key: &str, subject: &str, detail: &str) -> bool {
        self.note(INCIDENTS, &format!("{class}\t{key}\t{subject}\t{detail}"))
    }

    fn page(&self, text: &str) -> bool {
        self.note(PAGES, text)
    }
}

/// Runs the doctor over `read` until `bound` reads within its limit: the first round must find it over and apply a remedy, and every
/// later round must leave the proxy unpaged until the bound clears, within the doctor's patience.
pub fn heal(lander: &Lander, bound: Bound, read: &dyn Fn(u64) -> Facts) -> Result<String, String> {
    let hands = Drilled { lander };
    let mut book = Book::default();
    let first = cure(&read(facts::now()), &mut book, &hands, facts::now());
    let tripped: Vec<&Treated> = first.iter().filter(|treated| treated.finding.bound == bound && treated.finding.over).collect();
    if tripped.is_empty() {
        return Err(format!("the fault left {} within its limit:\n{}", bound.key(), render(&first)));
    }
    let outcomes: Vec<&String> = tripped.iter().flat_map(|treated| &treated.outcomes).collect();
    if let Some(failed) = outcomes.iter().find(|outcome| outcome.starts_with("failed ") || outcome.starts_with("unknown state")) {
        return Err(format!("{}: {failed}", bound.key()));
    }
    let applied = outcomes.iter().map(|outcome| outcome.as_str()).collect::<Vec<_>>().join("; ");
    let started = Instant::now();
    loop {
        let pages = hands.pages();
        if !pages.is_empty() {
            return Err(format!("the doctor paged the proxy: {}", pages.trim()));
        }
        let treated = cure(&read(facts::now()), &mut book, &hands, facts::now());
        let over: Vec<Treated> = treated.into_iter().filter(|treated| treated.finding.bound == bound && treated.finding.over).collect();
        if over.is_empty() {
            return Ok(format!("{} over its limit, the doctor {applied}; within its limit {:.1} s later", bound.key(), started.elapsed().as_secs_f64()));
        }
        if started.elapsed() > Duration::from_secs(PATIENCE_SECS) {
            return Err(format!("{} stays over {} s after the doctor {applied}:\n{}", bound.key(), PATIENCE_SECS, render(&over)));
        }
        sleep(POLL);
    }
}
