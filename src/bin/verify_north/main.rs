mod client;
mod exec;
mod janitor;
mod journey;
mod notes;
mod processes;
mod product;
mod report;
mod scratch;
mod transcript;

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use client::North;
use janitor::Janitor;
use journey::Plan;
use notes::{Note, Notes};

const USAGE: &str = "usage: verify-north [--north PATH] [--claude-account A]
Builds north from the checkout around the current directory, or uses --north PATH (a bare name is
looked up on PATH), then walks the spine end to end: `north land serve` serves a scratch repository,
a Claude haiku worker writes its change there and hands it in with one `hand-in` tool call,
the lander lands it on the scratch origin, and `north ledger worker` records the run; then it cleans up. Exits nonzero on any failure.
Defaults: the Claude account `north config agents plan --roomiest` names, the one with the most headroom that no reservation holds.";

struct Options {
    north: Option<String>,
    claude_account: Option<String>,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some(janitor::ARGUMENT) => janitor::serve(),
        Some("-h" | "--help") => {
            report::say(USAGE);
            ExitCode::SUCCESS
        }
        _ => match parse(&arguments) {
            Ok(options) => verify(&options),
            Err(problem) => {
                eprintln!("verify-north: {problem}\n{USAGE}");
                ExitCode::from(64)
            }
        },
    }
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { north: None, claude_account: None };
    let mut words = arguments.iter();
    while let Some(flag) = words.next() {
        let value = words.next().ok_or_else(|| format!("{flag} needs a value"))?.clone();
        match flag.as_str() {
            "--north" => options.north = Some(value),
            "--claude-account" => options.claude_account = Some(value),
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(options)
}

fn verify(options: &Options) -> ExitCode {
    let started = Instant::now();
    let state = match scratch::state_root() {
        Ok(state) => state,
        Err(problem) => {
            eprintln!("verify-north: {problem}");
            return ExitCode::FAILURE;
        }
    };
    let janitor = match Janitor::spawn(state.clone()) {
        Ok(janitor) => janitor,
        Err(error) => {
            let _ = fs::remove_dir_all(&state);
            eprintln!("verify-north: cannot start the cleanup process: {error}");
            return ExitCode::FAILURE;
        }
    };
    let notes = janitor.notes();
    notes.note(&Note::State(state.clone()));
    let failed = match prepare(options, &notes, &state) {
        Some(north) => journey::walk(north, notes, plan(options, state)),
        None => Some("0"),
    };
    let swept = janitor.finish();
    let failed = failed.or((!swept).then_some("7"));
    report::total(failed, started.elapsed());
    if failed.is_none() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn plan(options: &Options, state: std::path::PathBuf) -> Plan {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Plan { stamp, claude_account: options.claude_account.clone(), state }
}

fn prepare(options: &Options, notes: &Notes, state: &std::path::Path) -> Option<North> {
    let started = Instant::now();
    let title = if options.north.is_some() { "use the given north build" } else { "build north from this checkout" };
    let outcome = locate(options, notes, state);
    let line = outcome.as_ref().map(|north| north.program().display().to_string()).map_err(Clone::clone);
    report::step("0", title, &line, started.elapsed());
    outcome.ok()
}

fn locate(options: &Options, notes: &Notes, state: &std::path::Path) -> Result<North, String> {
    let here = env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let (program, directory) = if let Some(program) = &options.north {
        (product::installed(program)?, here)
    } else {
        let root = product::checkout(&here)?;
        (product::build(&root, notes)?, root)
    };
    notes.note(&Note::North(program.clone()));
    notes.note(&Note::Directory(directory.clone()));
    let north = North::new(program, directory, state.to_path_buf(), Some(notes.clone()));
    north.create_ledger()?;
    Ok(north)
}
