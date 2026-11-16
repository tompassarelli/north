//! `north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] [--notify] NAME -- CMD...`: records a scheduled job's
//! start and finish in NorthThreads' job_runs (name, host, started, finished, rc), runs CMD and
//! exits with its code. Recording never changes the job's result: the start record runs beside
//! CMD and stops when CMD ends, and the finish record, which writes the whole row, spools when the
//! ledger host is slow.
//!   --path DIRS  prepended to PATH for the records only (ssh, timeout); north's own directory is appended
//!   --user USER  when running as root, record as USER (runuser) so rows reach USER's ledger
//!   --class CLASS  run CMD as a moderate, heavy or gpu capacity lease held to its leased CPUs; a Rust compile is leased without it
//!   --scratch ROOT  give CMD the fresh directory ROOT/NAME as its TMPDIR, emptied first of what a killed run of NAME left, and remove it when CMD ends
//!   --notify     detach CMD into its own session and log, return at once, and spool JOB-START, a JOB-BEAT every
//!                60 s and JOB-EXIT to the fleet session $NORTH_FLEET_SESSION, so an agent can end its turn and
//!                wake on the result; it runs in its own systemd user scope when there is one, so the caller's
//!                unit stopping does not kill it; the JOB-START lease is the fleet's only record that the
//!                session waits on the job
mod detach;
mod records;
mod scratch;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use self::records::{Recorder, Spooler};
use self::scratch::Scratch;
use clap::Parser;
use north_capacity::capacity::{dev_shell, gate};
use north_foundation::foundation::clock::Timestamp;

const HELP: &str = "usage: north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] [--notify] NAME -- CMD...\nrecord a scheduled job's start and finish in NorthThreads' job_runs (name, host, started, finished, rc), run CMD and exit with its code.\n  --path DIRS  prepended to PATH for the records only (ssh, timeout)\n  --user USER  when running as root, record as USER (runuser) so rows reach USER's ledger\n  --class CLASS  run CMD through capacity admission as a moderate, heavy or gpu lease held to its leased CPUs (a Rust compile is admitted without it)\n  --scratch ROOT  give CMD the fresh directory ROOT/NAME as its TMPDIR and remove it when CMD ends\n  --notify     detach CMD into its own session and log, return at once, and spool JOB-START, a JOB-BEAT every 60 s and JOB-EXIT to the fleet session $NORTH_FLEET_SESSION";
const USAGE: &str = "usage: north job-run [--path DIRS] [--user USER] [--class CLASS] [--scratch ROOT] [--notify] NAME -- CMD...";
const FLEET_SESSION: &str = "NORTH_FLEET_SESSION";
const ADMITTED_CLASSES: [&str; 3] = ["moderate", "heavy", "gpu"];
const LEASE_SECONDS: &str = "3600";
const NOT_FOUND: u8 = 127;
const NOT_EXECUTABLE: u8 = 126;
const SIGNALED: i32 = 128;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Job {
    record_path: String,
    user: String,
    class: Option<String>,
    scratch: Option<PathBuf>,
    notify: Option<String>,
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
    #[arg(long, allow_hyphen_values = true)]
    path: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    user: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    class: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    scratch: Option<PathBuf>,
    #[arg(long)]
    notify: bool,
    #[arg(short = 'h', long)]
    help: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<String>,
}

pub(crate) fn parse(arguments: &[String], session: Option<&str>) -> Result<Parsed, String> {
    let words = Words::try_parse_from(arguments).map_err(|_| USAGE.to_owned())?;
    if words.help {
        return Ok(Parsed::Help);
    }
    if let Some(class) = &words.class
        && !ADMITTED_CLASSES.contains(&class.as_str())
    {
        return Err(format!("job-run: --class is one of {}", ADMITTED_CLASSES.join(", ")));
    }
    let notify = match words.notify {
        true => Some(session.filter(|session| !session.is_empty()).ok_or_else(|| format!("job-run: --notify needs {FLEET_SESSION}"))?.to_owned()),
        false => None,
    };
    match words.rest.as_slice() {
        [name, separator, command @ ..] if separator == "--" && !command.is_empty() => Ok(Parsed::Job(Job {
            record_path: words.path.unwrap_or_default(),
            user: words.user.unwrap_or_default(),
            class: words.class,
            scratch: words.scratch,
            notify,
            name: name.clone(),
            command: command.to_vec(),
        })),
        _ => Err(USAGE.to_owned()),
    }
}


fn stamp() -> String {
    let now = Timestamp::now();
    format!("{}.{:06}Z", now.iso().trim_end_matches('Z'), now.unix_nanos().rem_euclid(1_000_000_000) / 1000)
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
    let words = launch_words(&records::north(), job);
    let Some((program, arguments)) = words.split_first() else { return i32::from(NOT_FOUND) };
    let mut command = Command::new(program);
    command.args(arguments).env_remove(detach::LOG).stdin(Stdio::inherit());
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
    let session = env::var(FLEET_SESSION).ok();
    let job = match parse(arguments, session.as_deref()) {
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
    if let Some(session) = &job.notify
        && env::var_os(detach::LOG).is_none()
    {
        return detach::run(&job, arguments, session);
    }
    let code = serve(&job);
    u8::try_from(code & 0xff).unwrap_or(1)
}

fn serve(job: &Job) -> i32 {
    let host = north_base::platform::host::name();
    let recorder = Recorder::new(job, &host);
    let spooler = Spooler::new(job);
    let notified = job.notify.as_deref().map(|session| {
        let key = format!("{}@{host}#{}", job.name, std::process::id());
        let log = env::var(detach::LOG).unwrap_or_default();
        spooler.start(session, &key, &format!("job-run: {} started; output in {log}", job.name));
        let beating = spooler.beats(session, &key);
        (session.to_owned(), key, log, beating)
    });
    let started = stamp();
    let mut starting = recorder.start(&started);
    let scratch = job.scratch.as_deref().and_then(|root| Scratch::fresh(root, &job.name).inspect_err(|why| eprintln!("{why}; the job keeps the TMPDIR it was given")).ok());
    let code = run_command(job, scratch.as_ref().map(Scratch::path));
    drop(scratch);
    if let Some(starting) = starting.take() {
        starting.stop();
    }
    recorder.finish(&started, &stamp(), code);
    if let Some((session, key, log, beating)) = notified {
        if let Some(beating) = beating {
            beating.stop();
        }
        spooler.exit(&session, &key, code, &format!("job-run: {} exited {code}; output in {log}", job.name));
    }
    code
}
