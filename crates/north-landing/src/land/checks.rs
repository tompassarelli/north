use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;
use north_base::foundation::exit::{NO_VERDICT_STATUS, TRANSPORT_FAILED_TEXT};
use north_base::foundation::offload::{LANDER_SLOT_ENV, OFF_HOST_ENV, OWNER_ENV, RED_FIX_ENV};

pub use crate::platform::land_queue::ROUND;
use super::lander::Lander;
use super::refusal;

pub const SCRUBBED: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "SOPS_AGE_KEY", "SOPS_AGE_KEY_FILE", "SOPS_AGE_SSH_PRIVATE_KEY_FILE", "NORTH_RUNTIME_LINK"];
pub const BROKEN_BASE: &str = "main's check environment is broken: ";
pub const BASE_FAILS: &str = "and also fails on base tree";
pub const INFRASTRUCTURE: &str = "infrastructure: the check host is unfit: ";
const NORTH_MISSING: [&str; 4] = ["north: command not found", "north: not found", "north not on PATH", "north missing on PATH"];
const OFFLOAD: &str = "north capacity offload: ";
const OFFLOADED: &str = "safe-push: offloading to ";
const LAND_CHECK: &str = "safe-push: land check ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Refused(String),
    RedMain(String),
    Transient(String),
    BrokenBase(String),
    Deferred(String),
}

impl Verdict {
    pub fn class(&self) -> (&'static str, &str) {
        match self {
            Self::Green => ("green", ""),
            Self::RedMain(detail) => ("red-main", detail),
            Self::Transient(detail) => ("transient", detail),
            Self::Refused(detail) => ("refused", detail),
            Self::Deferred(detail) => ("deferred", detail),
            Self::BrokenBase(detail) => ("broken-base", detail),
        }
    }
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
    let offline_on_main_too = log.contains("--offline was specified") && log.contains(BASE_FAILS);
    let crate_missing = || lines().find(|line| offline_on_main_too && line.contains("failed to download"));
    let north_missing = || lines().find(|line| NORTH_MISSING.iter().any(|signature| line.contains(signature)));
    let exit = || lines().find(|line| line.contains(TRANSPORT_FAILED_TEXT));
    let transport = || exit().and_then(|exit| lines().rfind(|line| line.starts_with(OFFLOAD)).or_else(|| lines().any(|line| line.starts_with(OFFLOADED)).then_some(exit)));
    crate_missing().or_else(north_missing).or_else(transport).map(str::to_owned)
}

fn failing_check(log: &str) -> Option<String> {
    let lines = || log.lines().map(str::trim);
    let check = lines().find_map(|line| {
        let rest = line.strip_prefix(LAND_CHECK)?;
        if let Some((command, _)) = rest.split_once(" hung: ") {
            return Some(format!("land check {command} hung past its deadline"));
        }
        let (command, status) = rest.split_once(" exited ")?;
        Some(format!("land check {command} exited {}", status.split([';', ' ']).next()?))
    });
    check.or_else(|| lines().find(|line| refusal::gate(line)).and_then(|line| line.split_once(": ")).map(|(gate, _)| gate.to_owned()))
}

fn named(log: &str, detail: String) -> String {
    match failing_check(log) {
        Some(check) if !detail.starts_with(&check) => format!("{check}; log: {detail}"),
        _ => detail,
    }
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
}

pub struct Launch<'a> {
    pub checker: &'a [String],
    pub offload_owner: &'a str,
    pub round: String,
    pub red_fix: bool,
    pub lander_slot: bool,
    pub off_host: bool,
}

impl<'a> Launch<'a> {
    pub fn of(lander: &'a Lander) -> Self {
        Self { checker: &lander.checker, offload_owner: &lander.offload_owner, round: lander.round.load(Ordering::Relaxed).to_string(), red_fix: false, lander_slot: true, off_host: false }
    }
}

pub fn start(launch: &Launch<'_>, worktree: &Path, base: &str, log: &Path, own_group: bool) -> Result<Started, Verdict> {
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
    command.args(prefix).args(["--check-candidate", "--base", base]).current_dir(worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env("NORTH_LAND_SERVING", "1").env(OWNER_ENV, launch.offload_owner).env(ROUND, &launch.round);
    if launch.red_fix {
        command.env(RED_FIX_ENV, "1");
    }
    if launch.lander_slot {
        command.env(LANDER_SLOT_ENV, "1");
    }
    if launch.off_host {
        command.env(OFF_HOST_ENV, "1");
    }
    if let Some(logs) = log.parent() {
        command.env(super::trace::SPANS_ENV, super::trace::Spans::beside(logs).path());
    }
    if own_group {
        command.process_group(0);
    }
    for name in SCRUBBED {
        command.env_remove(name);
    }
    match command.spawn() {
        Ok(child) => Ok(Started { child, program: program.clone(), log: log.to_owned() }),
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
        let text = fs::read_to_string(log).unwrap_or_default();
        match parse(&text) {
            Some(Verdict::Refused(_)) if let Some(verdict) = unfit(log, &text) => verdict,
            Some(Verdict::Refused(_) | Verdict::Green) if status.code() == Some(NO_VERDICT_STATUS) => Verdict::Transient(format!("{program} exited {NO_VERDICT_STATUS}; log: {}", log.display())),
            Some(Verdict::Refused(detail)) => Verdict::Refused(named(&text, if detail.is_empty() { log.display().to_string() } else { detail })),
            Some(verdict) => verdict,
            None => Verdict::Transient(format!("{program} gave no verdict line ({status}); log: {}", log.display())),
        }
    }
}

pub fn run(lander: &Lander, worktree: &Path, base: &str, log: &Path) -> Verdict {
    match start(&Launch::of(lander), worktree, base, log, false) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    }
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod tests;
