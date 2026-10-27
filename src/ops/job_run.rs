use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::foundation::clock::Timestamp;
use crate::platform::harness::harness_root;

const RECORD_LIMIT: Duration = Duration::from_secs(30);
const LOG_ENV: &str = "JOB_RUN_LOG";
const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
const USAGE: &str = "usage: job-run [--path DIRS] [--user USER] [--notify] NAME -- CMD...";
const HELP: &str = "job-run [--path DIRS] [--user USER] [--notify] NAME -- CMD...: record a scheduled job's start and finish in
NorthThreads' job_runs (name, host, started, finished, rc), run CMD and exit with its code.
Recording never changes the job's result: the start record runs beside CMD and stops when CMD ends,
and the finish record, which writes the whole row, spools when the ledger host is slow.
  --path DIRS  prepended to PATH for the threads calls only (python3, ssh, timeout)
  --user USER  when running as root, record as USER (runuser) so rows reach USER's ledger
  --notify     detach CMD into its own session and log, return at once, and send its exit to the fleet session
               $NORTH_FLEET_SESSION, so an agent can end its turn and wake on the result";

struct Job {
    record_path: Option<String>,
    user: Option<String>,
    notify: bool,
    name: String,
    command: Vec<String>,
}

fn parse(args: &[String]) -> Result<Option<Job>, ()> {
    let (mut record_path, mut user, mut notify) = (None, None, false);
    let mut rest = args;
    while let [flag, tail @ ..] = rest {
        match (flag.as_str(), tail) {
            ("--path", [value, tail @ ..]) => (record_path, rest) = (Some(value.clone()), tail),
            ("--user", [value, tail @ ..]) => (user, rest) = (Some(value.clone()), tail),
            ("--notify", _) => (notify, rest) = (true, tail),
            ("-h" | "--help", _) => return Ok(None),
            _ => break,
        }
    }
    match rest {
        [name, separator, command @ ..] if separator == "--" && !command.is_empty() => {
            Ok(Some(Job { record_path, user, notify, name: name.clone(), command: command.to_vec() }))
        }
        _ => Err(()),
    }
}

fn stamp() -> String {
    let micros = Timestamp::now().iso_micros_utc_offset();
    format!("{}Z", micros.strip_suffix("+00:00").unwrap_or(&micros))
}

fn host() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_default()
}

fn home_of(user: &str) -> String {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    passwd
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|fields| fields.first() == Some(&user))
        .and_then(|fields| fields.get(5).map(|home| (*home).to_owned()))
        .unwrap_or_else(|| format!("/home/{user}"))
}

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| "north".into())
}

impl Job {
    fn recorder_path(&self) -> OsString {
        let mut path = OsString::new();
        if let Ok(root) = harness_root() {
            path.push(root.join("bin"));
            path.push(":");
        }
        if let Some(extra) = &self.record_path {
            path.push(extra);
            path.push(":");
        }
        path.push(std::env::var_os("PATH").unwrap_or_default());
        path
    }

    fn as_user(&self, arguments: &[&str]) -> Command {
        let path = self.recorder_path();
        let mut command = match self.user.as_deref().filter(|_| rustix::process::geteuid().is_root()) {
            Some(user) => {
                let mut command = Command::new("runuser");
                command.args(["-u", user, "--", "env"]).arg(format!("HOME={}", home_of(user))).arg(format!("USER={user}"));
                let mut assignment = OsString::from("PATH=");
                assignment.push(&path);
                command.arg(assignment).arg(north());
                command
            }
            None => Command::new(north()),
        };
        command.env("PATH", &path).env_remove(LOG_ENV).args(arguments);
        command.process_group(0).stdin(Stdio::null());
        command
    }

    fn recorder(&self, host: &str, extra: &[&str]) -> Command {
        let mut arguments = vec!["threads", "job-run", &self.name, "--host", host];
        arguments.extend_from_slice(extra);
        self.as_user(&arguments)
    }

    fn log_path(&self) -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        let moment: String = Timestamp::now().iso().chars().filter(|c| !matches!(c, '-' | ':' | 'Z')).collect();
        state.join("north/jobs").join(format!("{}-{moment}-{}.log", self.name.replace('/', "_"), std::process::id()))
    }
}

fn record(mut command: Command, stop: &AtomicBool) -> Option<ExitStatus> {
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() >= RECORD_LIMIT {
            if let Some(group) = i32::try_from(child.id()).ok().and_then(rustix::process::Pid::from_raw) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Term);
            }
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn exit_code(status: ExitStatus) -> u8 {
    status.code().map_or_else(|| 128 + status.signal().unwrap_or(0), |code| code & 0xff) as u8
}

fn run_job(command: &[String]) -> u8 {
    match Command::new(&command[0]).args(&command[1..]).env_remove(LOG_ENV).status() {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("job-run: cannot run {}: {error}", command[0]);
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}

fn detach(job: &Job, args: &[String], session: &str) -> u8 {
    let log = job.log_path();
    let started = log.parent().map(fs::create_dir_all).transpose().and_then(|_| File::create(&log));
    let spawned = started.and_then(|file| {
        Command::new(north())
            .arg("job-run")
            .args(args)
            .env(LOG_ENV, &log)
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(file)
            .spawn()
    });
    if spawned.is_err() {
        eprintln!("job-run: could not detach {}; see {}", job.name, log.display());
        return 1;
    }
    println!("job-run: {} runs detached with its output in {}; its exit arrives as a fleet message to {session}, so end your turn.", job.name, log.display());
    0
}

fn notify(job: &Job, session: &str, rc: u8, log: &str) {
    let text = format!("job-run: {} exited {rc}; output in {log}", job.name);
    let sent = record(job.as_user(&["fleet", "send", session, &text]), &AtomicBool::new(false));
    if !sent.is_some_and(|status| status.success()) {
        eprintln!("job-run: sending {}'s exit to {session} failed", job.name);
    }
}

fn execute(job: &Job, log: Option<&str>) -> u8 {
    let (host, started) = (host(), stamp());
    let mut start = job.recorder(&host, &["--started", &started]);
    start.stdout(Stdio::null()).stderr(Stdio::null());
    let stop = Arc::new(AtomicBool::new(false));
    let starting = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || record(start, &stop))
    };
    let rc = run_job(&job.command);
    stop.store(true, Ordering::Relaxed);
    let mut finish = job.recorder(&host, &["--started", &started, "--finished", &stamp(), "--rc", &rc.to_string()]);
    finish.stdout(Stdio::null());
    if !record(finish, &AtomicBool::new(false)).is_some_and(|status| status.success()) {
        eprintln!("job-run: recording {} finish failed; the job's result is unaffected", job.name);
    }
    let _ = starting.join();
    if let (true, Some(log), Ok(session)) = (job.notify, log, std::env::var(SESSION_ENV)) {
        notify(job, &session, rc, log);
    }
    rc
}

pub(crate) fn run(args: &[String]) -> u8 {
    let job = match parse(args) {
        Ok(Some(job)) => job,
        Ok(None) => {
            println!("{HELP}");
            return 0;
        }
        Err(()) => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let log = std::env::var(LOG_ENV).ok().filter(|log| !log.is_empty());
    if job.notify && std::env::var(SESSION_ENV).unwrap_or_default().is_empty() {
        eprintln!("job-run: --notify needs {SESSION_ENV}");
        return 2;
    }
    match (job.notify, &log) {
        (true, None) => detach(&job, args, &std::env::var(SESSION_ENV).unwrap_or_default()),
        (true, Some(log)) => {
            let _ = rustix::process::setsid();
            execute(&job, Some(log))
        }
        _ => execute(&job, None),
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn flags_precede_the_name_and_the_command_follows_a_separator() {
        let job = parse(&words("--path /a:/b --user tom --notify nightly -- sh -c true")).unwrap().unwrap();
        assert_eq!((job.record_path.as_deref(), job.user.as_deref(), job.name.as_str(), job.notify), (Some("/a:/b"), Some("tom"), "nightly", true));
        assert_eq!(job.command, words("sh -c true"));
        assert!(parse(&words("-h")).unwrap().is_none());
        for bad in ["nightly", "nightly --", "nightly sh -c true", "--path", "--path /a nightly -- "] {
            assert!(parse(&words(bad)).is_err(), "{bad}");
        }
    }
}
