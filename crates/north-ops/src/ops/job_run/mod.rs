//! `north job-run [--path DIRS] [--user USER] [--notify] NAME -- CMD...`: records a scheduled job's
//! start and finish in NorthThreads' job_runs (name, host, started, finished, rc), runs CMD and
//! exits with its code. Recording never changes the job's result: the start record runs beside
//! CMD and stops when CMD ends, and the finish record, which writes the whole row, spools when the
//! ledger host is slow.
//!   --path DIRS  prepended to PATH for the records only (ssh, timeout); north's own directory is appended
//!   --user USER  when running as root, record as USER (runuser) so rows reach USER's ledger
//!   --notify     detach CMD into its own session and log, return at once, and spool JOB-START, a JOB-BEAT every
//!                60 s and JOB-EXIT to the fleet session $NORTH_FLEET_SESSION, so an agent can end its turn and
//!                wake on the result; it runs in its own systemd user scope when there is one, so the caller's
//!                unit stopping does not kill it; the JOB-START lease is the fleet's only record that the
//!                session waits on the job
mod detach;
mod records;

use std::env;
use std::process::{Command, ExitStatus, Stdio};

use self::records::{Recorder, Spooler};
use crate::capacity::dev_shell;
use crate::foundation::clock::Timestamp;

const HELP: &str = "usage: north job-run [--path DIRS] [--user USER] [--notify] NAME -- CMD...\nrecord a scheduled job's start and finish in NorthThreads' job_runs (name, host, started, finished, rc), run CMD and exit with its code.\n  --path DIRS  prepended to PATH for the records only (ssh, timeout)\n  --user USER  when running as root, record as USER (runuser) so rows reach USER's ledger\n  --notify     detach CMD into its own session and log, return at once, and spool JOB-START, a JOB-BEAT every 60 s and JOB-EXIT to the fleet session $NORTH_FLEET_SESSION";
const USAGE: &str = "usage: north job-run [--path DIRS] [--user USER] [--notify] NAME -- CMD...";
const FLEET_SESSION: &str = "NORTH_FLEET_SESSION";
const NOT_FOUND: u8 = 127;
const NOT_EXECUTABLE: u8 = 126;
const SIGNALED: i32 = 128;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Job {
    record_path: String,
    user: String,
    notify: Option<String>,
    name: String,
    command: Vec<String>,
}

pub(crate) enum Parsed {
    Help,
    Job(Job),
}

pub(crate) fn parse(arguments: &[String], session: Option<&str>) -> Result<Parsed, String> {
    let mut rest = arguments;
    let (mut record_path, mut user, mut notify) = (String::new(), String::new(), None);
    while let Some((flag, tail)) = rest.split_first() {
        match flag.as_str() {
            "--path" | "--user" => {
                let (value, tail) = tail.split_first().ok_or_else(|| USAGE.to_owned())?;
                if flag == "--path" { record_path.clone_from(value) } else { user.clone_from(value) }
                rest = tail;
            }
            "--notify" => {
                notify = Some(session.filter(|session| !session.is_empty()).ok_or_else(|| format!("job-run: --notify needs {FLEET_SESSION}"))?.to_owned());
                rest = tail;
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            _ => break,
        }
    }
    match rest {
        [name, separator, command @ ..] if separator == "--" && !command.is_empty() => {
            Ok(Parsed::Job(Job { record_path, user, notify, name: name.clone(), command: command.to_vec() }))
        }
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

fn run_command(command: &[String]) -> i32 {
    let words = dev_shell::enter(&records::north(), command);
    let Some((program, arguments)) = words.split_first() else { return i32::from(NOT_FOUND) };
    match Command::new(program).args(arguments).env_remove(detach::LOG).stdin(Stdio::inherit()).status() {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("job-run: {program}: {error}");
            i32::from(if error.kind() == std::io::ErrorKind::NotFound { NOT_FOUND } else { NOT_EXECUTABLE })
        }
    }
}

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
    let host = crate::platform::host::name();
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
    let code = run_command(&job.command);
    if let Some(mut starting) = starting.take() {
        records::stop(&mut starting);
    }
    recorder.finish(&started, &stamp(), code);
    if let Some((session, key, log, beating)) = notified {
        if let Some(mut beating) = beating {
            records::stop(&mut beating);
        }
        spooler.exit(&session, &key, code, &format!("job-run: {} exited {code}; output in {log}", job.name));
    }
    code
}
