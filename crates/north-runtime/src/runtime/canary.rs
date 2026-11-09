use super::layout::{self, Layout};
use super::lifecycle::{self, Canarying, Canary};
use super::failure::DEFERRED;
use super::update;
use north_base::foundation::exit::NO_VERDICT;
use std::time::Duration;
use probes::Probe;

mod launch;
mod probes;

pub(super) use launch::probe as launch_probe;

const TAIL_LINES: usize = 12;
const TAIL_BYTES: usize = 1200;
const USAGE: u8 = 64;
const NOT_EXECUTABLE: u8 = 126;
const NOT_FOUND: u8 = 127;
const REMOTE_ROLLBACK_LIMIT: Duration = Duration::from_secs(180);

pub(super) enum Ran {
    Exited { code: u8, log: String },
    Unrun(String),
}

pub(super) enum Rollback {
    To(String),
    Deferred(String),
    Failed(String),
}

pub(super) trait Machine: Sync {
    fn probe(&self, probe: &Probe) -> Ran;
    fn roll_back(&self, sha: &str) -> Rollback;
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
    let instance = format!("{host}/{sha}");
    if lifecycle::state_of::<Canarying>(layout, &instance).ok().flatten().is_some_and(Canary::judged) {
        return Verdict::Skipped;
    }
    let short = layout::short(sha);
    let finish = |state: Canary, cause: &str| {
        if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, state, cause) {
            eprintln!("north runtime canary: {host}: {short}: {}", failure.message);
        }
    };
    if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, Canary::Judging, "running the core-loop probes on the host") {
        println!("north runtime canary: {host}: {short} not judged here: {}", failure.message);
        return Verdict::Skipped;
    }
    let probes = probes::core_loop(host, sha);
    let ran: Vec<Ran> = std::thread::scope(|scope| {
        let handles: Vec<_> = probes.iter().map(|probe| scope.spawn(|| machine.probe(probe))).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Ran::Unrun("the probe panicked".to_owned()))).collect()
    });
    let failures: Vec<String> = probes.iter().zip(&ran).filter_map(|(probe, ran)| match ran {
        Ran::Exited { code: 0, .. } | Ran::Unrun(_) => None,
        Ran::Exited { code, log } => Some(format!("probe `{}` exited {code}; log tail: {}", probe.name, tail(log))),
    }).collect();
    if failures.is_empty() {
        let unrun: Vec<String> = probes.iter().zip(&ran).filter_map(|(probe, ran)| match ran {
            Ran::Unrun(why) => Some(format!("probe `{}` not run: {why}", probe.name)),
            Ran::Exited { .. } => None,
        }).collect();
        if unrun.is_empty() {
            finish(Canary::Passed, &format!("all {} core-loop probes exited 0", probes.len()));
            println!("north runtime canary: {host}: {short} passed {} core-loop probes", probes.len());
            return Verdict::Passed;
        }
        let why = unrun.join("; ");
        println!("north runtime canary: {host}: {short} not judged: {why}");
        finish(Canary::Unjudged, &why);
        return Verdict::Unrun(why);
    }
    let failure = format!("{} on {host} for runtime {short}", failures.join("; "));
    match machine.roll_back(sha) {
        Rollback::To(previous) => {
            finish(Canary::RolledBack, &failure);
            trust(layout, host, &previous, short);
            machine.alarm(&format!("north runtime canary: {failure}; {host} rolled back to {} and holds {short} back for an hour", layout::short(&previous)));
            Verdict::RolledBack
        }
        Rollback::Deferred(why) => {
            let why = format!("{failure}; rollback deferred: {why}");
            println!("north runtime canary: {host}: {short} not judged: {why}");
            finish(Canary::Unjudged, &why);
            Verdict::Unrun(why)
        }
        Rollback::Failed(why) => {
            finish(Canary::Stuck, &format!("{failure}; could not roll back: {why}"));
            machine.alarm(&format!("north runtime canary: {failure}; {host} could not roll back: {why}"));
            Verdict::Stuck
        }
    }
}

fn trust(layout: &Layout, host: &str, previous: &str, failed: &str) {
    let instance = format!("{host}/{previous}");
    if !layout::is_sha(previous) || lifecycle::state_of::<Canarying>(layout, &instance).ok().flatten().is_some_and(Canary::judged) {
        return;
    }
    if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, Canary::Trusted, &format!("the rollback target after {failed} failed; trusted, not re-judged")) {
        eprintln!("north runtime canary: {host}: {}: {}", layout::short(previous), failure.message);
    }
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
        Some(code @ (USAGE | NOT_EXECUTABLE | NOT_FOUND | NO_VERDICT)) => Ran::Unrun(format!("the judged runtime could not run the probe (exit {code}): {}", tail(&log))),
        Some(code) => Ran::Exited { code, log },
        None => Ran::Unrun(format!("the probe was killed by a signal: {}", tail(&log))),
    }
}

pub(super) struct Live<'a> {
    pub(super) layout: &'a Layout,
    pub(super) host: &'a super::probe::Shell,
}

impl Machine for Live<'_> {
    fn probe(&self, probe: &Probe) -> Ran {
        match self.host.exec(&probe.script, probe.limit) {
            Ok(Some(output)) => classify(output.status.code(), format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
            Ok(None) => Ran::Unrun(format!("no verdict within {}s", probe.limit.as_secs())),
            Err(error) => Ran::Unrun(format!("could not start: {error}")),
        }
    }

    fn roll_back(&self, sha: &str) -> Rollback {
        if let super::probe::Shell::Here(_) = self.host {
            return match update::roll_back(self.layout, sha) {
                Ok(previous) => Rollback::To(previous),
                Err(failure) if failure.code == DEFERRED => Rollback::Deferred(failure.message),
                Err(failure) => Rollback::Failed(failure.message),
            };
        }
        let script = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/current/north\" runtime rollback --from {sha}\n");
        match self.host.exec(&script, REMOTE_ROLLBACK_LIMIT) {
            Ok(Some(output)) => match output.status.code().and_then(|code| u8::try_from(code).ok()) {
                Some(0) => Rollback::To(String::from_utf8_lossy(&output.stdout).trim().rsplit(' ').next().unwrap_or_default().to_owned()),
                Some(DEFERRED) => Rollback::Deferred(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
                _ => Rollback::Failed(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
            },
            Ok(None) => Rollback::Failed(format!("no answer within {}s", REMOTE_ROLLBACK_LIMIT.as_secs())),
            Err(error) => Rollback::Failed(format!("could not start: {error}")),
        }
    }

    fn alarm(&self, text: &str) {
        update::alert_proxy(text);
    }
}

#[cfg(test)]
mod tests;
