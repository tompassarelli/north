//! `north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] NAME -- CMD...`: runs a scheduled job's
//! CMD and exits with its code.
//!   --path DIRS, --user USER  accepted and unused since the job ledger and the fleet spool they served went (north#1650); nixos-config's job units still pass them
//!   --class CLASS  run CMD as a moderate, heavy or gpu capacity lease held to its leased CPUs; a Rust compile is leased without it
//!   --scratch ROOT  give CMD the fresh directory ROOT/NAME as its TMPDIR, emptied first of what a killed run of NAME left, and remove it when CMD ends
mod scratch;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use self::scratch::Scratch;
use clap::Parser;
use north_capacity::capacity::{dev_shell, gate};

const HELP: &str = "usage: north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] NAME -- CMD...\nrun a scheduled job's CMD and exit with its code.\n  --path DIRS, --user USER  accepted and unused since the job ledger went (north#1650)\n  --class CLASS  run CMD through capacity admission as a moderate, heavy or gpu lease held to its leased CPUs (a Rust compile is admitted without it)\n  --scratch ROOT  give CMD the fresh directory ROOT/NAME as its TMPDIR and remove it when CMD ends";
const USAGE: &str = "usage: north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] NAME -- CMD...";
const ADMITTED_CLASSES: [&str; 3] = ["moderate", "heavy", "gpu"];
const LEASE_SECONDS: &str = "3600";
const NOT_FOUND: u8 = 127;
const NOT_EXECUTABLE: u8 = 126;
const SIGNALED: i32 = 128;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Job {
    class: Option<String>,
    scratch: Option<PathBuf>,
    name: String,
    command: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Parsed {
    Help,
    Job(Job),
}

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
struct Words {
    #[arg(long = "path", allow_hyphen_values = true)]
    _path: Option<String>,
    #[arg(long = "user", allow_hyphen_values = true)]
    _user: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    class: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    scratch: Option<PathBuf>,
    #[arg(short = 'h', long)]
    help: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<String>,
}

pub(crate) fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let words = Words::try_parse_from(arguments).map_err(|_| USAGE.to_owned())?;
    if words.help {
        return Ok(Parsed::Help);
    }
    if let Some(class) = &words.class
        && !ADMITTED_CLASSES.contains(&class.as_str())
    {
        return Err(format!("job-run: --class is one of {}", ADMITTED_CLASSES.join(", ")));
    }
    match words.rest.as_slice() {
        [name, separator, command @ ..] if separator == "--" && !command.is_empty() => Ok(Parsed::Job(Job { class: words.class, scratch: words.scratch, name: name.clone(), command: command.to_vec() })),
        _ => Err(USAGE.to_owned()),
    }
}

fn north() -> String {
    env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string())
}

fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().unwrap_or_else(|| SIGNALED + status.signal().unwrap_or(0))
}

/// The words that start the job: a leased job goes through `north capacity host run`, which enters the dev shell inside its own scope, and any other runs in the dev shell directly.
pub(crate) fn launch_words(north: &str, job: &Job) -> Vec<String> {
    let class = job.class.as_deref().or_else(|| gate::class_of(&job.command, 0).map(gate::Class::name));
    let Some(class) = class else { return dev_shell::enter(north, &job.command) };
    let mut words = [north, "capacity", "host", "run", "--class", class, "--owner"].map(str::to_owned).to_vec();
    words.extend([format!("job-run:{}", job.name), "--timeout-seconds".to_owned(), LEASE_SECONDS.to_owned(), "--".to_owned()]);
    words.extend(job.command.iter().cloned());
    words
}

fn run_command(job: &Job, scratch: Option<&Path>) -> i32 {
    let words = launch_words(&north(), job);
    let Some((program, arguments)) = words.split_first() else { return i32::from(NOT_FOUND) };
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::inherit());
    if let Some(scratch) = scratch {
        command.env("TMPDIR", scratch);
    }
    match command.status() {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("job-run: {program}: {error}");
            i32::from(if error.kind() == std::io::ErrorKind::NotFound { NOT_FOUND } else { NOT_EXECUTABLE })
        }
    }
}

#[cfg(test)]
mod usage_tests;

pub fn run(arguments: &[String]) -> u8 {
    let job = match parse(arguments) {
        Ok(Parsed::Job(job)) => job,
        Ok(Parsed::Help) => {
            println!("{HELP}");
            return 0;
        }
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let scratch = job.scratch.as_deref().and_then(|root| Scratch::fresh(root, &job.name).inspect_err(|why| eprintln!("{why}; the job keeps the TMPDIR it was given")).ok());
    let code = run_command(&job, scratch.as_ref().map(Scratch::path));
    drop(scratch);
    u8::try_from(code & 0xff).unwrap_or(1)
}
