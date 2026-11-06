use std::path::{Path, PathBuf};

use north_base::foundation::step::{Refused, Step, cancelled};

use super::plan::Gate;
use super::verdict::{Outcome, Scanned, Snapshot, reuse};
use crate::shell::Shell;
use crate::shell::processes::Child;

pub fn scan(arguments: &[&str]) -> Step {
    cancelled()?;
    let arguments: Vec<String> = arguments.iter().map(|&argument| argument.to_owned()).collect();
    match north_landing::land::scans::run(&arguments) {
        0 => cancelled(),
        code => Err(Refused(code)),
    }
}

pub fn publication_scan(base: &str, head: &str) -> u8 {
    north_landing::land::scans::run(&["publication".to_owned(), base.to_owned(), head.to_owned()])
}

pub fn sync_dependencies(root: &Path) -> Result<(), String> {
    north_landing::land::slots::deps::sync(root)
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Capacity {
    pub command: String,
    pub offload_host: Option<String>,
}

impl Capacity {
    fn environment(&self) -> Vec<(&'static str, String)> {
        let mut environment = vec![("SAFE_PUSH_LAND_CAPACITY", self.command.clone())];
        environment.extend(self.offload_host.iter().map(|host| ("SAFE_PUSH_LAND_OFFLOAD_HOST", host.clone())));
        environment
    }
}

pub struct LandChecks<'a> {
    pub base: &'a str,
    pub head: &'a str,
    pub dry_run: bool,
    pub destination: &'a str,
    pub event_log: &'a Path,
    pub wait_event: &'a str,
    pub capacity: &'a Capacity,
    pub claimed: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Event {
    pub stage: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub events: Vec<Event>,
    pub skip: Option<String>,
    pub red_main: Option<String>,
    pub ran: bool,
}

impl Report {
    pub fn read(outcome: &str) -> Self {
        let mut report = Self::default();
        for line in outcome.lines() {
            let mut fields = line.split('\t');
            let (kind, first, second, third) = (fields.next(), fields.next(), fields.next(), fields.next());
            match (kind, first) {
                (Some("event"), Some(stage)) => report.events.push(Event {
                    stage: stage.to_owned(),
                    status: second.unwrap_or_default().to_owned(),
                    detail: third.unwrap_or_default().to_owned(),
                }),
                (Some("skip"), Some(detail)) => report.skip = Some(detail.to_owned()),
                (Some("red-main"), Some(base)) => report.red_main = Some(base.to_owned()),
                (Some("ran"), _) => report.ran = true,
                _ => {}
            }
        }
        report
    }
}

pub struct LandChecked {
    pub step: Step,
    pub report: Report,
}

impl LandChecked {
    pub fn outcome(&self) -> Outcome {
        match (&self.step, &self.report.skip) {
            (Ok(()), _) => Outcome::Passed,
            (Err(_), Some(skip)) => Outcome::failed_with(skip.clone()),
            (Err(_), None) => Outcome::failed(),
        }
    }
}

pub struct Gates<'a> {
    shell: &'a Shell,
    root: PathBuf,
}

impl<'a> Gates<'a> {
    pub fn new(shell: &'a Shell, root: &Path) -> Self {
        Self { shell, root: root.to_path_buf() }
    }

    pub fn land_checks_present(&self) -> bool {
        !self.shell.git.setting("HEAD", "land.check").is_empty() || self.north(&["land", "checks", "--any", "--head", "HEAD"]).is_some()
    }

    pub fn capacity(&self, to: &str) -> Capacity {
        let command = self.shell.git.setting("HEAD", "landing.capacity");
        let offloaded = self.shell.git.setting("HEAD", "landing.offloaded-capacity");
        let offload_host = if offloaded.is_empty() { None } else { self.offload_host(to) };
        match offload_host {
            Some(host) => Capacity { command: offloaded, offload_host: Some(host) },
            None => Capacity { command, offload_host: None },
        }
    }

    fn offload_host(&self, to: &str) -> Option<String> {
        if !self.shell.settings.land_offload || !self.land_checks_present() {
            return None;
        }
        self.north(&["land", "checks", "--portable", "--base", &format!("refs/remotes/origin/{to}"), "--head", "HEAD"])?;
        self.north(&["capacity", "offload", "choose"]).filter(|host| !host.is_empty())
    }

    fn north(&self, arguments: &[&str]) -> Option<String> {
        self.shell.processes.succeeds(&self.shell.processes.current_exe(), &self.root, arguments)
    }

    pub fn land_checks(&self, request: &LandChecks) -> LandChecked {
        let arguments: Vec<String> = [
            "land",
            "checks",
            "--base",
            request.base,
            "--head",
            request.head,
            "--dry",
            if request.dry_run { "1" } else { "0" },
            "--safe-push",
            &self.shell.settings.safe_push,
            "--destination",
            request.destination,
            "--event-log",
            &request.event_log.to_string_lossy(),
            "--wait-event",
            request.wait_event,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let program = self.shell.processes.current_exe();
        let mut environment = request.capacity.environment();
        environment.push(("SAFE_PUSH_OUTPUT_CLAIMED", request.claimed.to_owned()));
        let child = Child { program: &program, arguments: &arguments, directory: &self.root, environment: &environment };
        let finished = self.shell.processes.run_with_outcome(&child);
        LandChecked { step: finished.step, report: Report::read(&finished.outcome) }
    }

    pub fn land_checks_beside<S, N>(&self, request: &LandChecks, at: Snapshot, scans: S, now: N) -> (LandChecked, Vec<(Gate, Outcome)>)
    where
        S: Fn() -> Vec<(Gate, Outcome)> + Sync,
        N: FnOnce() -> Snapshot,
    {
        std::thread::scope(|scope| {
            let background = scope.spawn(|| Scanned { at, results: scans() });
            let checked = self.land_checks(request);
            let results = match background.join() {
                Ok(scanned) => reuse(&scanned, &now()).map_or_else(&scans, <[_]>::to_vec),
                Err(_) => scans(),
            };
            (checked, results)
        })
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
