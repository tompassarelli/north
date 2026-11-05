use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use rustix::process::{Pid, Signal, kill_process};

use super::Job;

const RECORD_SECONDS: &str = "30";
const DEFAULT_BEAT_SECONDS: &str = "60";

pub(super) fn north() -> String {
    env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string())
}

pub(super) fn search_path(record_path: &str) -> String {
    let own = env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let ahead = own.iter().map(|directory| directory.display().to_string()).chain((!record_path.is_empty()).then(|| record_path.to_owned()));
    ahead.chain(env::var("PATH").ok()).collect::<Vec<_>>().join(":")
}

pub(super) fn find(program: &str, path: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env::split_paths(path)
        .map(|directory| directory.join(program))
        .find(|candidate| std::fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0))
}

fn home_of(user: &str) -> String {
    std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            (fields.first() == Some(&user)).then(|| fields.get(5).map(|home| (*home).to_owned())).flatten()
        })
        .unwrap_or_else(|| format!("/home/{user}"))
}

struct Launcher {
    user: String,
    path: String,
}

impl Launcher {
    fn new(job: &Job) -> Self {
        Self { user: job.user.clone(), path: search_path(&job.record_path) }
    }

    fn command(&self, program: &str, arguments: &[String]) -> Command {
        let as_user = !self.user.is_empty() && rustix::process::geteuid().is_root();
        let mut command = Command::new(if as_user { "runuser" } else { program });
        command.env("PATH", &self.path).stdin(Stdio::null()).stdout(Stdio::null());
        if as_user {
            command.args(["-u", &self.user, "--", "env", &format!("PATH={}", self.path), &format!("HOME={}", home_of(&self.user)), &format!("USER={}", self.user), program]);
        }
        command.args(arguments);
        command
    }
}

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

pub(super) fn signal(child: &mut Child, signal: Signal) {
    if let Some(pid) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
        let _ = kill_process(pid, signal);
    }
}

pub(super) fn stop(child: &mut Child) {
    signal(child, Signal::Term);
    let _ = child.wait();
}

pub(super) struct Recorder<'a> {
    launcher: Launcher,
    job: &'a Job,
    host: &'a str,
}

impl<'a> Recorder<'a> {
    pub(super) fn new(job: &'a Job, host: &'a str) -> Self {
        Self { launcher: Launcher::new(job), job, host }
    }

    fn command(&self, rest: &[&str]) -> Command {
        let mut arguments = words(&[RECORD_SECONDS, &north(), "threads", "job-run", &self.job.name, "--host", self.host]);
        arguments.extend(words(rest));
        self.launcher.command("timeout", &arguments)
    }

    pub(super) fn start(&self, started: &str) -> Option<Child> {
        self.command(&["--started", started]).stderr(Stdio::null()).spawn().ok()
    }

    pub(super) fn finish(&self, started: &str, finished: &str, code: i32) {
        let recorded = self.command(&["--started", started, "--finished", finished, "--rc", &code.to_string()]).status();
        if !recorded.is_ok_and(|status| status.success()) {
            eprintln!("job-run: recording {} finish failed; the job's result is unaffected", self.job.name);
        }
    }
}

pub(super) struct Spooler<'a> {
    launcher: Launcher,
    job: &'a Job,
}

impl<'a> Spooler<'a> {
    pub(super) fn new(job: &'a Job) -> Self {
        Self { launcher: Launcher::new(job), job }
    }

    fn command(&self, rest: &[&str]) -> Command {
        let mut arguments = words(&["fleet", "spool", "job"]);
        arguments.extend(words(rest));
        self.launcher.command(&north(), &arguments)
    }

    fn ran(&self, rest: &[&str], what: &str) {
        if !self.command(rest).status().is_ok_and(|status| status.success()) {
            eprintln!("job-run: spooling {}'s {what} failed", self.job.name);
        }
    }

    pub(super) fn start(&self, session: &str, key: &str, text: &str) {
        self.ran(&["start", session, key, text], "start");
    }

    pub(super) fn beats(&self, session: &str, key: &str) -> Option<Child> {
        let every = env::var("JOB_RUN_BEAT_SECS").unwrap_or_else(|_| DEFAULT_BEAT_SECONDS.to_owned());
        let pid = std::process::id().to_string();
        let beating = self.command(&["beats", session, key, &every, &pid]).stderr(Stdio::null()).spawn();
        if beating.is_err() {
            eprintln!("job-run: spooling {}'s beats failed", self.job.name);
        }
        beating.ok()
    }

    pub(super) fn exit(&self, session: &str, key: &str, code: i32, text: &str) {
        self.ran(&["exit", session, key, &code.to_string(), text], &format!("exit for {session}"));
    }
}
