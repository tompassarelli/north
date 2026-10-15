mod client;
mod exec;
mod janitor;
mod journey;
mod notes;
mod processes;
mod product;
mod report;
mod terminal;
mod transcript;

use std::env;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use client::North;
use janitor::Janitor;
use journey::Plan;
use notes::{Note, Notes};

const USAGE: &str = "usage: verify-north [--north PATH] [--claude-account A] [--codex-account A]
Builds north from the checkout around the current directory, or uses --north PATH (a bare name is
looked up on PATH), then drives the fleet TUI and CLI end to end and cleans up. Exits nonzero on
any failure. Defaults: --claude-account c, --codex-account pooled.";

struct Options {
    north: Option<String>,
    claude_account: String,
    codex_account: String,
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
    let mut options = Options { north: None, claude_account: "c".into(), codex_account: "pooled".into() };
    let mut words = arguments.iter();
    while let Some(flag) = words.next() {
        let value = words.next().ok_or_else(|| format!("{flag} needs a value"))?.clone();
        match flag.as_str() {
            "--north" => options.north = Some(value),
            "--claude-account" => options.claude_account = value,
            "--codex-account" => options.codex_account = value,
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(options)
}

fn verify(options: &Options) -> ExitCode {
    let started = Instant::now();
    let janitor = match Janitor::spawn() {
        Ok(janitor) => janitor,
        Err(error) => {
            eprintln!("verify-north: cannot start the cleanup process: {error}");
            return ExitCode::FAILURE;
        }
    };
    let notes = janitor.notes();
    let failed = match prepare(options, &notes) {
        Some(north) => journey::walk(north, notes, plan(options)),
        None => Some("0"),
    };
    let swept = janitor.finish();
    let failed = failed.or((!swept).then_some("7"));
    report::total(failed, started.elapsed());
    if failed.is_none() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn plan(options: &Options) -> Plan {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Plan { stamp, claude_account: options.claude_account.clone(), codex_account: options.codex_account.clone() }
}

fn prepare(options: &Options, notes: &Notes) -> Option<North> {
    let started = Instant::now();
    let title = if options.north.is_some() { "use the given north build" } else { "build north from this checkout" };
    let outcome = locate(options, notes);
    let line = outcome.as_ref().map(|north| north.program().display().to_string()).map_err(Clone::clone);
    report::step("0", title, &line, started.elapsed());
    outcome.ok()
}

fn locate(options: &Options, notes: &Notes) -> Result<North, String> {
    let here = env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let (program, directory) = if let Some(program) = &options.north {
        (product::installed(program)?, here)
    } else {
        let root = product::checkout(&here)?;
        (product::build(&root, notes)?, root)
    };
    notes.note(&Note::North(program.clone()));
    notes.note(&Note::Directory(directory.clone()));
    Ok(North::new(program, directory, Some(notes.clone())))
}
