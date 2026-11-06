use super::layout::{self, Layout};
use super::update;
use crate::pacing::placement::{config, gate};
use crate::platform::process;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const CHECK: &str = "true";
const BOUND: Duration = Duration::from_secs(5 * 60);
const CHECK_SECONDS: u64 = 4 * 60;
const KEPT: usize = 50;
const TAIL_LINES: usize = 12;
const TAIL_BYTES: usize = 1200;
const UNREACHED: [u8; 4] = [255, 96, 97, 75];

pub(super) enum Ran {
    Exited { code: u8, log: String },
    Unrun(String),
}

pub(super) trait Machine {
    fn offload(&self, sha: &str) -> Ran;
    fn roll_back(&self, sha: &str) -> Result<String, String>;
    fn alarm(&self, text: &str);
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Skipped,
    Passed,
    Unrun(String),
    RolledBack,
    Stuck,
}

pub(super) fn check(layout: &Layout, host: &str, sha: &str, machine: &impl Machine) -> Verdict {
    if canaried(layout, host, sha) {
        return Verdict::Skipped;
    }
    let short = layout::short(sha);
    let (code, log) = match machine.offload(sha) {
        Ran::Unrun(why) => {
            println!("north runtime canary: {host}: {short} not judged: {why}");
            return Verdict::Unrun(why);
        }
        Ran::Exited { code: 0, .. } => {
            record(layout, host, sha);
            println!("north runtime canary: {host}: {short} passed the land check `{CHECK}`");
            return Verdict::Passed;
        }
        Ran::Exited { code, log } => (code, log),
    };
    let failure = format!("land check `{CHECK}` on {host} exited {code} for runtime {short}; log tail: {}", tail(&log));
    match machine.roll_back(sha) {
        Ok(previous) => {
            machine.alarm(&format!("north runtime canary: {failure}; {host} rolled back to {previous} and holds {short} back for an hour"));
            Verdict::RolledBack
        }
        Err(why) => {
            record(layout, host, sha);
            machine.alarm(&format!("north runtime canary: {failure}; {host} could not roll back: {why}"));
            Verdict::Stuck
        }
    }
}

fn canaried(layout: &Layout, host: &str, sha: &str) -> bool {
    let wanted = format!("{host} {sha}");
    fs::read_to_string(layout.canaried()).is_ok_and(|seen| seen.lines().any(|line| line == wanted))
}

fn record(layout: &Layout, host: &str, sha: &str) {
    let mut lines: Vec<String> = fs::read_to_string(layout.canaried()).unwrap_or_default().lines().map(str::to_owned).collect();
    lines.push(format!("{host} {sha}"));
    let kept = &lines[lines.len().saturating_sub(KEPT)..];
    let _ = fs::write(layout.canaried(), kept.iter().map(|line| format!("{line}\n")).collect::<String>());
}

pub(super) fn tail(log: &str) -> String {
    let lines: Vec<&str> = log.lines().map(str::trim_end).filter(|line| !line.is_empty()).collect();
    let joined = lines[lines.len().saturating_sub(TAIL_LINES)..].join(" | ");
    let start = joined.len().saturating_sub(TAIL_BYTES);
    let start = (start..=joined.len()).find(|at| joined.is_char_boundary(*at)).unwrap_or(joined.len());
    joined[start..].to_owned()
}

pub(super) fn classify(code: Option<i32>, log: String) -> Ran {
    match code.and_then(|code| u8::try_from(code).ok()) {
        Some(code) if UNREACHED.contains(&code) => Ran::Unrun(format!("the offload did not reach a slot (exit {code}): {}", tail(&log))),
        Some(code) => Ran::Exited { code, log },
        None => Ran::Unrun(format!("the offload was killed by a signal: {}", tail(&log))),
    }
}

pub(super) struct Live<'a> {
    pub(super) layout: &'a Layout,
    pub(super) checkout: &'a Path,
    pub(super) host: &'a super::probe::Shell,
}

impl Live<'_> {
    fn machine(&self) -> Option<String> {
        let name = self.host.name();
        let cfg = config().ok()?;
        gate::machines(&cfg).into_iter().find(|machine| machine.name == name || machine.ssh == name).map(|machine| machine.name)
    }

    fn parent(&self, sha: &str) -> Result<String, String> {
        let output = process::run("git", Some(self.checkout), &["rev-parse", "--verify", "--quiet", &format!("{sha}^")]).map_err(|error| format!("git: {error}"))?;
        let parent = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        match output.status.success() && layout::is_sha(&parent) {
            true => Ok(parent),
            false => Err(format!("{sha} has no parent commit in {}", self.checkout.display())),
        }
    }
}

impl Machine for Live<'_> {
    fn offload(&self, sha: &str) -> Ran {
        let Some(machine) = self.machine() else {
            return Ran::Unrun("no [hosts.NAME] with an ssh destination to offload to".to_owned());
        };
        let base = match self.parent(sha) {
            Ok(base) => base,
            Err(why) => return Ran::Unrun(why),
        };
        let Ok(north) = std::env::current_exe() else {
            return Ran::Unrun("this north has no executable path".to_owned());
        };
        let mut command = Command::new(north);
        command
            .current_dir(self.checkout)
            .args(["capacity", "offload", "run", "--host", &machine, "--base", &base, "--head", sha, "--timeout", &CHECK_SECONDS.to_string(), "--", CHECK]);
        match process::try_run_command_within(command, None, BOUND) {
            Ok(Some(output)) => classify(output.status.code(), format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
            Ok(None) => Ran::Unrun(format!("no verdict within {}s", BOUND.as_secs())),
            Err(error) => Ran::Unrun(format!("could not start: {error}")),
        }
    }

    fn roll_back(&self, sha: &str) -> Result<String, String> {
        match self.host {
            super::probe::Shell::Here(_) => update::roll_back(self.layout, sha).map_err(|failure| failure.message),
            shell => shell.run(&format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/current/north\" runtime rollback --from {sha}\n")).map(|out| out.trim().rsplit(' ').next().unwrap_or_default().to_owned()),
        }
    }

    fn alarm(&self, text: &str) {
        update::alert_proxy(text);
    }
}

#[cfg(test)]
mod tests;
