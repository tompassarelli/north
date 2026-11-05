use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;

use super::lander::Lander;

pub const SCRUBBED: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "SOPS_AGE_KEY", "SOPS_AGE_KEY_FILE", "SOPS_AGE_SSH_PRIVATE_KEY_FILE", "NORTH_RUNTIME_LINK"];
pub const BROKEN_BASE: &str = "main's check environment is broken: ";
pub const ROUND: &str = "NORTH_LAND_ROUND";
pub const RECHECK: &str = "NORTH_LAND_RECHECK";
pub const INFRASTRUCTURE: &str = "infrastructure: the check host is unfit: ";
const NORTH_MISSING: [&str; 4] = ["north: command not found", "north: not found", "north not on PATH", "north missing on PATH"];
const OFFLOAD: &str = "north capacity offload: ";
const OFFLOAD_EXIT: &str = "exited 255";
const OFFLOADED: &str = "safe-push: offloading to ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Refused(String),
    RedMain(String),
    Transient(String),
    BrokenBase(String),
    Deferred(String),
}

pub fn parse(text: &str) -> Option<Verdict> {
    let line = text.lines().rev().find_map(|line| line.trim_end().strip_prefix("safe-push: verdict "))?;
    let (class, detail) = line.split_once(' ').unwrap_or((line, ""));
    let detail = detail.trim().to_owned();
    Some(match class {
        "green" => Verdict::Green,
        "refused" => Verdict::Refused(detail),
        "red-main" => Verdict::RedMain(detail),
        "transient" if detail.starts_with(BROKEN_BASE) => Verdict::BrokenBase(detail),
        "transient" => Verdict::Transient(detail),
        "deferred" => Verdict::Deferred(detail),
        other => Verdict::Transient(format!("unknown verdict class {other}")),
    })
}

pub fn unfit_host(log: &str) -> Option<String> {
    let lines = || log.lines().map(str::trim);
    let offline = log.contains("--offline was specified");
    let crate_missing = || lines().find(|line| offline && line.contains("failed to download"));
    let north_missing = || lines().find(|line| NORTH_MISSING.iter().any(|signature| line.contains(signature)));
    let exit = || lines().find(|line| line.contains(OFFLOAD_EXIT));
    let transport = || exit().and_then(|exit| lines().rfind(|line| line.starts_with(OFFLOAD)).or_else(|| lines().any(|line| line.starts_with(OFFLOADED)).then_some(exit)));
    crate_missing().or_else(north_missing).or_else(transport).map(str::to_owned)
}

pub fn infrastructure(detail: &str) -> bool {
    detail.starts_with(INFRASTRUCTURE)
}

fn unfit(log: &Path, text: &str) -> Option<Verdict> {
    unfit_host(text).map(|signature| Verdict::Transient(format!("{INFRASTRUCTURE}{signature}; log: {}", log.display())))
}

pub struct Started {
    child: Child,
    program: String,
    log: PathBuf,
    by_exit: bool,
}

pub struct Launch<'a> {
    pub checker: &'a [String],
    pub offload_owner: &'a str,
    pub round: String,
    pub recheck: bool,
}

impl<'a> Launch<'a> {
    pub fn of(lander: &'a Lander) -> Self {
        Self { checker: &lander.checker, offload_owner: &lander.offload_owner, round: lander.round.load(Ordering::Relaxed).to_string(), recheck: false }
    }
}

pub fn start(launch: &Launch<'_>, worktree: &Path, base: &str, log: &Path, own_group: bool, runner: Option<&str>) -> Result<Started, Verdict> {
    let Some((program, prefix)) = launch.checker.split_first() else {
        return Err(Verdict::Transient("no checker configured".to_owned()));
    };
    if let Some(parent) = log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(Verdict::Transient(format!("{}: {error}", parent.display())));
    }
    let (stdout, stderr) = match File::create(log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Err(Verdict::Transient(format!("{}: {error}", log.display()))),
    };
    let mut command = Command::new(program);
    command.args(prefix).args(["--check-candidate", "--base", base]).current_dir(worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env("NORTH_LAND_SERVING", "1").env("NORTH_OFFLOAD_OWNER", launch.offload_owner).env(ROUND, &launch.round);
    if launch.recheck {
        command.env(RECHECK, "1");
    } else {
        command.env_remove(RECHECK);
    }
    if own_group {
        command.process_group(0);
    }
    for name in SCRUBBED {
        command.env_remove(name);
    }
    if let Some(runner) = runner {
        command.env("SAFE_PUSH_LAND_RUNNER", runner);
    }
    match command.spawn() {
        Ok(child) => Ok(Started { child, program: program.clone(), log: log.to_owned(), by_exit: false }),
        Err(error) => Err(Verdict::Transient(format!("{program}: {error}"))),
    }
}

impl Started {
    pub fn poll(&mut self) -> Option<Verdict> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(self.verdict(status)),
            Ok(None) => None,
            Err(error) => Some(Verdict::Transient(format!("{}: {error}", self.program))),
        }
    }

    pub fn wait(mut self) -> Verdict {
        match self.child.wait() {
            Ok(status) => self.verdict(status),
            Err(error) => Verdict::Transient(format!("{}: {error}", self.program)),
        }
    }

    pub fn kill(mut self) {
        if let Some(pid) = rustix::process::Pid::from_raw(self.child.id().cast_signed()) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn verdict(&self, status: ExitStatus) -> Verdict {
        let (program, log) = (&self.program, &self.log);
        if self.by_exit && status.success() {
            return Verdict::Green;
        }
        let text = fs::read_to_string(log).unwrap_or_default();
        if self.by_exit {
            return unfit(log, &text).unwrap_or_else(|| Verdict::Refused(format!("{program} {status}; log: {}", log.display())));
        }
        match parse(&text) {
            Some(Verdict::Refused(_)) if let Some(verdict) = unfit(log, &text) => verdict,
            Some(Verdict::Refused(_) | Verdict::Green) if status.code() == Some(75) => Verdict::Transient(format!("{program} exited 75; log: {}", log.display())),
            Some(Verdict::Refused(detail)) if detail.is_empty() => Verdict::Refused(log.display().to_string()),
            Some(verdict) => verdict,
            None => Verdict::Transient(format!("{program} gave no verdict line ({status}); log: {}", log.display())),
        }
    }
}

pub fn run(lander: &Lander, worktree: &Path, base: &str, log: &Path) -> Verdict {
    match start(&Launch::of(lander), worktree, base, log, false, None) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    }
}

pub fn start_judged_by_exit(argv: &[String], worktree: &Path, log: &Path) -> Result<Started, Verdict> {
    let Some((program, arguments)) = argv.split_first() else {
        return Err(Verdict::Transient("no command to run".to_owned()));
    };
    if let Some(parent) = log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(Verdict::Transient(format!("{}: {error}", parent.display())));
    }
    let (stdout, stderr) = match File::create(log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Err(Verdict::Transient(format!("{}: {error}", log.display()))),
    };
    let mut command = Command::new(program);
    command.args(arguments).current_dir(worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).process_group(0);
    for name in SCRUBBED {
        command.env_remove(name);
    }
    match command.spawn() {
        Ok(child) => Ok(Started { child, program: program.clone(), log: log.to_owned(), by_exit: true }),
        Err(error) => Err(Verdict::Transient(format!("{program}: {error}"))),
    }
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod tests;
