use std::path::Path;

use super::super::clippy_first;
use super::super::judged;
use super::super::unit_reuse;
use super::super::verdict_key;
use super::line_refusals::Refusals;
use super::log::{first_error, last_error};
use super::{Arguments, Config, Environment, NO_NORTH, Stop, affected, block_on, run_added_avoiding, say, shared_build, steps, warn};
use north_foundation::foundation::exit::{ALREADY_PLACED_STATUS, NO_VERDICT_STATUS, SLOTS_BUSY_STATUS};
use north_base::platform::finish_record::acceptance_in_prequeue;
use north_base::platform::harness::{DEFAULTS, document};

const ATTEMPTS: usize = 3;
const FAILING: &str = "; failing check `";
const TESTS_AFTER_LANDING: &str = "tests_after_landing";
/// The line window the checks before queueing wait in per placement; a spent window starts over, so the hand-in waits for a slot until its command ends, such as a lead's 30-minute background cap (north#1508).
const LINE_SECONDS: u64 = 180;

/// The checks a lane runs alone before its ticket is written. With `tests_after_landing` on, only the `@enqueue` land.check
/// lines that compile nothing, run on this host with the installed north (north#1247), and the crates the lane changes
/// and the crates that depend on them compile on the workers through the shared store (north#1631); its clippy and every line that builds run after it lands. Off, its clippy and every `@enqueue` line run on a build host. No test runs here (north#1508).
pub fn queue_commands(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    if let Some(refusal) = judged::own_refusal(root, base, head) {
        return Err(refusal);
    }
    let config = judged::config(root, base, head);
    let landing = config.after_set().commands(root, base, head)?;
    let after_landing = tests_after_landing();
    let clippy = (!after_landing).then(|| clippy_first::listed(&landing, |command| !config.is_inferred(command))).flatten();
    let queued_lines = config.queue_set(north_diff_gates::landing_refusals::enforcing()).commands(root, base, head)?;
    let unread = !landing.iter().any(|command| verdict_key::reads_it(std::slice::from_ref(command)));
    let untested = if unread { config.commands(root, base, head)?.into_iter().filter(|command| !unit_reuse::runs_tests(command)).collect() } else { Vec::new() };
    let mut commands: Vec<String> = clippy.into_iter().collect();
    for command in queued_lines.iter().chain(&untested).map(|command| shared_build::built(command)) {
        if !(after_landing && shared_build::compiles(&command)) && !commands.contains(&command) {
            commands.push(command);
        }
    }
    if after_landing {
        let built = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.arguments());
        if !built.is_empty() {
            commands.push(format!("north land graph-build --changed {built}"));
        }
    }
    Ok(commands)
}

/// Why the checks before queueing refuse a lane: one of them ran and failed, or none reached a verdict, which says
/// nothing about the lane's change (north#1233).
#[derive(Debug)]
pub enum QueueRefusal {
    Failed(String),
    Unjudged(String),
}

impl QueueRefusal {
    pub fn into_text(self) -> String {
        match self {
            Self::Failed(text) | Self::Unjudged(text) => text,
        }
    }
}

pub fn refuse_queue_checks(root: &Path, base: &str, head: &str, north: &str) -> Result<(), String> {
    judge_queue_checks(root, base, head, north).map_err(QueueRefusal::into_text)
}

pub fn judge_queue_checks(root: &Path, base: &str, head: &str, north: &str) -> Result<(), QueueRefusal> {
    judge_queue_checks_until(root, base, head, north, &Stop::new()).map(drop)
}

pub fn judge_queue_checks_until(root: &Path, base: &str, head: &str, north: &str, stop: &Stop) -> Result<Vec<String>, QueueRefusal> {
    let judged = judge(root, (base, head), None, (north, stop));
    judged.run.map(|()| judged.failed_on_main)
}

/// What became of a finish call's acceptance command in a run before queueing (north#1036): it passed, its own run
/// failed, an earlier failure cancelled it before it ran, or no run reached a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accepted {
    Passed,
    Failed,
    NotRun,
    Unjudged,
}

/// The verdict of a run before queueing that also ran a finish call's acceptance command (north#1036): the run's own,
/// the acceptance command's, read from its keyed verdict and from the log a failed run of it kept, and that log.
#[derive(Debug)]
pub struct Judged {
    pub run: Result<(), QueueRefusal>,
    pub accepted: Accepted,
    pub output: String,
    /// The checks the lane fails with main failing them too, which the run accepts (north#1631).
    pub failed_on_main: Vec<String>,
}

impl Judged {
    fn unjudged(text: String) -> Self {
        Self { run: Err(QueueRefusal::Unjudged(text)), accepted: Accepted::Unjudged, output: String::new(), failed_on_main: Vec::new() }
    }

    /// Whether the finish call passes: its acceptance command passed and no check before queueing failed (north#1233).
    pub fn passed(&self) -> bool {
        self.run.is_ok() && self.accepted == Accepted::Passed
    }

    /// The exit code a finish check run this way reports: 0 when it passed, 75 when no run reached a verdict, else 1.
    pub fn status(&self) -> u8 {
        match (&self.run, self.accepted) {
            (Ok(()), Accepted::Passed) => 0,
            (Err(QueueRefusal::Unjudged(_)), _) | (_, Accepted::Unjudged) => u8::try_from(NO_VERDICT_STATUS).unwrap_or(1),
            _ => 1,
        }
    }
}

/// Whether a finish call's acceptance `command` joins the run before queueing `head` on `base` (north#1036): the
/// switch `finish_acceptance_in_prequeue` is on, the repository runs no check on GitHub Actions, and the command and
/// every check before queueing are portable, so a build host runs the whole run and none of it runs on this host.
pub fn joins_queue_run(root: &Path, base: &str, head: &str, command: &str) -> bool {
    let config = judged::config(root, base, head);
    let portable = |command: &str| config.local_reason(command.trim()).is_none();
    acceptance_in_prequeue() && !command.trim().is_empty() && config.workflow.is_empty() && portable(command) && queue_commands(root, base, head).is_ok_and(|commands| commands.iter().all(|command| portable(command)))
}

/// Runs the checks before queueing with a finish call's acceptance `command` as one more check of the same run, on
/// `head`'s tree and the host the run is placed on, so each verdict is keyed as the lander's own and the safe-push the
/// lane is then handed to reuses the whole run (north#1036, north#1230). Given `on`, a build host its caller chose, the
/// run is placed from there, never from a host the head's `.safe-push` names, and every check goes through the offload
/// (spread over the hosts the caller's placement names, this host's own offload slots among them); it reaches no
/// verdict when any of its checks would run outside the offload.
pub fn judge_with_acceptance(root: &Path, base: &str, head: &str, command: &str, on: Option<&str>, north: &str) -> Judged {
    judge(root, (base, head), Some((command.trim(), on.unwrap_or_default())), (north, &Stop::new()))
}

fn judge(root: &Path, (base, head): (&str, &str), acceptance: Option<(&str, &str)>, (north, stop): (&str, &Stop)) -> Judged {
    if north.is_empty() {
        return Judged::unjudged(NO_NORTH.to_owned());
    }
    steps::begin();
    let (config, mut commands) = match steps::timed("commands", || listed(root, base, head)) {
        Ok(listed) => listed,
        Err(text) => return Judged::unjudged(text),
    };
    let (command, host) = acceptance.unzip();
    let host = host.unwrap_or_default();
    let (placed, environment) = placement(&config, north, command.is_none());
    if let Some(command) = command.filter(|command| !commands.iter().any(|listed| listed.trim() == *command)) {
        commands.push(command.to_owned());
    }
    if commands.is_empty() {
        return Judged { run: Ok(()), accepted: Accepted::Unjudged, output: String::new(), failed_on_main: Vec::new() };
    }
    if !host.is_empty() && (!config.workflow.is_empty() || commands.iter().any(|command| config.local_reason(command).is_some())) {
        return Judged::unjudged(format!("a check before queueing would run on this host rather than on {host}, so the run does not start"));
    }
    say(&format!("safe-push: running {} check(s) before queueing the lane: {}", commands.len(), commands.join("; ")));
    let outcome = tempfile::NamedTempFile::new().ok();
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), one_lane: true, queueing: true, runner_host: host.to_owned(), outcome: outcome.as_ref().map(|file| file.path().to_owned()), ..Arguments::default() };
    let (status, passed) = match attempts(root, &arguments, (&placed, &environment), &commands, (command, stop)) {
        Ok(ran) => ran,
        Err(text) => return Judged::unjudged(text),
    };
    let (own, others) = command.zip(outcome.as_ref()).map(|(command, outcome)| failed_output(outcome.path(), command)).unwrap_or_default();
    let failing = outcome.as_ref().and_then(|outcome| failing_step(outcome.path(), status));
    let run = if status == 0 { Ok(()) } else { Err(refusal(status, (root, base, head), failing)) };
    let failed_on_main = outcome.as_ref().map(|outcome| not_gating(outcome.path())).unwrap_or_default();
    Judged { accepted: accepted(&run, passed, own.is_some()), output: own.unwrap_or(others), run, failed_on_main }
}

/// The checks the run accepted because main fails them the same way, as the run's outcome names them.
fn not_gating(outcome: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(outcome).unwrap_or_default();
    text.lines().filter_map(|line| line.strip_prefix("event\tland check\tnot gating\t")?.split_once(": ").map(|(command, _)| command.to_owned())).collect()
}

fn placement(config: &Config, north: &str, alone: bool) -> (Config, Environment) {
    if alone && tests_after_landing() {
        return (config.without_placement(), Environment::local(north.to_owned()));
    }
    (config.clone(), Environment::load(north.to_owned()))
}

type Listed = (Config, Vec<String>);

fn listed(root: &Path, base: &str, head: &str) -> Result<Listed, String> {
    let config = judged::config(root, base, head);
    Ok((config, queue_commands(root, base, head)?))
}

/// The run's exit status and whether `watched` passed in it, running it again on another host when one reached no verdict, up to `ATTEMPTS` times, and again on another line whenever a slot line refuses it; why it cannot run when no build host answers.
fn attempts(root: &Path, arguments: &Arguments, (config, environment): (&Config, &Environment), commands: &[String], (watched, stop): (Option<&str>, &Stop)) -> Result<(i32, Option<bool>), String> {
    let mut failed: Vec<String> = Vec::new();
    let (mut status, mut passed) = (NO_VERDICT_STATUS, None);
    let mut attempt = 1;
    let mut lines = Refusals::within(LINE_SECONDS);
    while attempt <= ATTEMPTS {
        let arguments = Arguments { line_within: Some(lines.left()), ..arguments.clone() };
        let ran = block_on(run_added_avoiding(root.to_owned(), arguments, (config.clone(), environment.clone()), commands.to_vec(), watched, &lines.avoiding(&failed), stop)).map_err(|error| error.to_string())?;
        (status, passed) = (ran.status, ran.accepted);
        if stop.stopped() {
            return Err(super::STOPPED.to_owned());
        }
        if ran.unanswered {
            return Err("no build host answers over ssh, so the checks before queueing cannot run; the lane is not queued uncompiled, so run safe-push again once a build host is back".to_owned());
        }
        if status == SLOTS_BUSY_STATUS {
            block_on(lines.place_again(&ran.host, status)).map_err(|error| error.to_string())?;
            continue;
        }
        if status != NO_VERDICT_STATUS {
            break;
        }
        retry(ran.host, attempt, &mut failed);
        attempt += 1;
    }
    Ok((status, passed))
}

/// Says the next attempt after a run on `host` reached no verdict, and leaves that host out of it.
fn retry(host: String, attempt: usize, failed: &mut Vec<String>) {
    if attempt < ATTEMPTS {
        warn(&format!("safe-push: the checks before queueing reached no verdict on {}; running them again ({} of {ATTEMPTS})", if host.is_empty() { "this host" } else { host.as_str() }, attempt + 1));
    }
    if !host.is_empty() {
        failed.push(host);
    }
}

fn accepted(run: &Result<(), QueueRefusal>, passed: Option<bool>, logged: bool) -> Accepted {
    match (run, passed) {
        (Err(QueueRefusal::Unjudged(_)), _) | (_, None) => Accepted::Unjudged,
        (_, Some(true)) => Accepted::Passed,
        (_, Some(false)) if logged => Accepted::Failed,
        (_, Some(false)) => Accepted::NotRun,
    }
}

fn failed_output(outcome: &Path, command: &str) -> (Option<String>, String) {
    let text = std::fs::read_to_string(outcome).unwrap_or_default();
    let failed: Vec<(&str, String)> = text.lines().filter_map(|line| line.strip_prefix("failed-step\t")?.split_once('\t')).map(|(failed, log)| (failed, std::fs::read_to_string(log).unwrap_or_default())).collect();
    let own = failed.iter().find(|(failed, _)| [command.to_owned(), shared_build::built(command)].iter().any(|form| form == failed.trim())).map(|(_, log)| log.clone());
    (own, failed.iter().map(|(failed, log)| format!("the check before queueing `{failed}` failed:\n{log}")).collect())
}

/// The first check the run recorded as failed with the first error line of its log, or its last when the run reached no verdict, since a check that gives up says why last; how the run exited when the log holds none.
fn failing_step(outcome: &Path, status: i32) -> Option<String> {
    let text = std::fs::read_to_string(outcome).unwrap_or_default();
    let (command, log) = text.lines().find_map(|line| line.strip_prefix("failed-step\t")?.split_once('\t'))?;
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let said = if status == NO_VERDICT_STATUS { last_error(&text) } else { first_error(&text) };
    let first = said.unwrap_or_else(|| format!("exited {status}"));
    Some(format!("{FAILING}{command}`: {first}"))
}

/// The failing check and its first error line that a refusal text names, as `refusal` wrote them.
pub fn failing(refusal: &str) -> Option<(&str, &str)> {
    refusal.split_once(FAILING)?.1.split_once("`: ")
}

fn refusal(status: i32, (root, base, head): (&Path, &str, &str), failing: Option<String>) -> QueueRefusal {
    if status == ALREADY_PLACED_STATUS {
        return QueueRefusal::Unjudged("the same checks of this head already wait or run on a build host, named above, so this run was refused rather than queued a second time; the lane is not queued uncompiled, so run safe-push again once they end".to_owned());
    }
    if status == NO_VERDICT_STATUS {
        return QueueRefusal::Unjudged(match failing {
            Some(step) => format!("the checks before queueing reached no verdict {ATTEMPTS} times, and the last run named the check that gave up{step}; the lane is not queued uncompiled, so run safe-push again once that check can decide"),
            None => format!("the checks before queueing reached no verdict {ATTEMPTS} times (exit 75: the host or its connection failed before any check reported); the lane is not queued uncompiled, so run safe-push again"),
        });
    }
    let rerun = if failing.as_deref().and_then(self::failing).is_some_and(|(command, _)| command.contains("cargo clippy")) {
        let crates = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.changed_arguments());
        format!("; to rerun clippy alone: cargo clippy --all-targets {crates} -- -D warnings")
    } else {
        String::new()
    };
    QueueRefusal::Failed(format!("a check before queueing exited {status}; fix the failure above{rerun}{}{}", off_base(root, base, head), failing.unwrap_or_default()))
}

fn off_base(root: &Path, base: &str, head: &str) -> String {
    let contained = std::process::Command::new("git").arg("-C").arg(root).args(["merge-base", "--is-ancestor", base, head]).output().map_or(true, |output| output.status.code() != Some(1));
    if contained {
        return String::new();
    }
    format!("; the checks ran against base {base}, which the lane's history does not contain: the lane stays on its older base when it cannot be rebased in place; rebase it onto {base} and hand in again")
}

pub fn tests_after_landing() -> bool {
    document(DEFAULTS).get(TESTS_AFTER_LANDING).and_then(|entry| entry.get("value")).and_then(toml_edit::Item::as_str) == Some("on")
}

#[cfg(test)]
#[path = "queue_checks_tests.rs"]
mod tests;
