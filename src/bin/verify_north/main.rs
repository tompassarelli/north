mod client;
mod exec;
mod janitor;
mod journey;
mod login;
mod notes;
mod processes;
mod product;
mod report;
mod scratch;

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use client::North;
use janitor::Janitor;
use journey::Plan;
use notes::{Note, Notes};

const USAGE: &str = "usage: verify-north [--north PATH] [--claude-account A [--from-claude-login]]
Builds north from the checkout around the current directory, or uses --north PATH (a bare name is
looked up on PATH), then walks the spine end to end: `north land serve` serves a scratch repository,
a confined worker command runs `safe-push --check-only` and `north hand-in` through the judged outside broker,
and the lander lands the commit on the scratch origin; then it cleans up. Exits nonzero on any failure.
--from-claude-login makes account A's profile under ~/.local/state/north/profiles/claude-A from Claude Code's login in ~/.claude when it has none.";

struct Options {
    north: Option<String>,
    claude_account: Option<String>,
    from_claude_login: bool,
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
    let mut options = Options { north: None, claude_account: None, from_claude_login: false };
    let mut words = arguments.iter();
    while let Some(flag) = words.next() {
        if flag == "--from-claude-login" {
            options.from_claude_login = true;
            continue;
        }
        let value = words.next().ok_or_else(|| format!("{flag} needs a value"))?.clone();
        match flag.as_str() {
            "--north" => options.north = Some(value),
            "--claude-account" => options.claude_account = Some(value),
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    if options.from_claude_login && options.claude_account.is_none() {
        return Err("--from-claude-login needs --claude-account".to_owned());
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
        Some(north) => journey::walk(north, notes, plan(state)),
        None => Some("0"),
    };
    let swept = janitor.finish();
    let failed = failed.or((!swept).then_some("4"));
    report::total(failed, started.elapsed());
    if failed.is_none() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn plan(state: std::path::PathBuf) -> Plan {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Plan { stamp, state }
}

fn prepare(options: &Options, notes: &Notes, state: &std::path::Path) -> Option<North> {
    if let (true, Some(label)) = (options.from_claude_login, &options.claude_account) {
        let started = Instant::now();
        let home = std::path::PathBuf::from(env::var_os("HOME").unwrap_or_default());
        let outcome = login::profile(&home, label);
        report::step("0", "make the Claude account's profile from Claude Code's login", &outcome, started.elapsed());
        outcome.ok()?;
    }
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
    Ok(North::new(program, directory, state.to_path_buf(), Some(notes.clone())))
}
