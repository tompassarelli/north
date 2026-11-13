use std::path::Path;

use super::super::clippy_first;
use super::super::verdict_key;
use super::{Arguments, Config, affected, block_on, run_added_avoiding, say, shared_build, steps, warn};
use north_base::foundation::exit::{NO_VERDICT_STATUS, SLOTS_BUSY_STATUS};
use north_base::platform::finish_record::acceptance_in_prequeue;

const ATTEMPTS: usize = 3;

/// The checks a lane runs alone before its ticket is written: its clippy (build and link), the
/// `@enqueue` land.check lines, and the land test selection the batch would otherwise run, or the
/// land.check lines themselves when none of them reads a test selection, so a lane is never vouched
/// for on tests that did not run (north#1230). Read without running them, so a test can pin the set
/// (north#892).
pub fn queue_commands(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let config = Config::load(root, head);
    let landing = config.after_set().commands(root, base, head)?;
    let clippy = clippy_first::listed(&landing, |command| !config.is_inferred(command));
    let queued_lines = config.queue_set(crate::landing_refusals::enforcing()).commands(root, base, head)?;
    let queued: Vec<String> = queued_lines.iter().map(|command| shared_build::built(command)).collect();
    let tests = match lane_tests(&landing) {
        Some(selected) => vec![selected],
        None => config.commands(root, base, head)?.iter().map(|command| shared_build::built(command)).collect(),
    };
    let mut commands: Vec<String> = clippy.into_iter().collect();
    for command in queued.into_iter().chain(tests) {
        if !commands.contains(&command) {
            commands.push(command);
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

pub fn refuse_queue_checks(root: &Path, base: &str, head: &str) -> Result<(), String> {
    judge_queue_checks(root, base, head).map_err(QueueRefusal::into_text)
}

pub fn judge_queue_checks(root: &Path, base: &str, head: &str) -> Result<(), QueueRefusal> {
    judge(root, base, head, None).run
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
}

impl Judged {
    fn unjudged(text: String) -> Self {
        Self { run: Err(QueueRefusal::Unjudged(text)), accepted: Accepted::Unjudged, output: String::new() }
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
    let config = Config::load(root, head);
    let portable = |command: &str| config.local_reason(command.trim()).is_none();
    acceptance_in_prequeue() && !command.trim().is_empty() && config.workflow.is_empty() && portable(command) && queue_commands(root, base, head).is_ok_and(|commands| commands.iter().all(|command| portable(command)))
}

/// Runs the checks before queueing with a finish call's acceptance `command` as one more check of the same run, on
/// `head`'s tree and the host the run is placed on, so each verdict is keyed as the lander's own and the safe-push the
/// lane is then handed to reuses the whole run (north#1036, north#1230). Given `on`, a build host its caller chose, the
/// run is placed from there, never from a host the head's `.safe-push` names, and every check goes through the offload
/// (spread over the hosts the caller's placement names, this host's own offload slots among them); it reaches no
/// verdict when any of its checks would run outside the offload.
pub fn judge_with_acceptance(root: &Path, base: &str, head: &str, command: &str, on: Option<&str>) -> Judged {
    judge(root, base, head, Some((command.trim(), on.unwrap_or_default())))
}

fn judge(root: &Path, base: &str, head: &str, acceptance: Option<(&str, &str)>) -> Judged {
    steps::begin();
    let (config, clippy, mut commands) = match steps::timed("commands", || listed(root, base, head)) {
        Ok(listed) => listed,
        Err(text) => return Judged::unjudged(text),
    };
    let (command, host) = acceptance.unzip();
    let host = host.unwrap_or_default();
    if let Some(command) = command.filter(|command| !commands.iter().any(|listed| listed.trim() == *command)) {
        commands.push(command.to_owned());
    }
    if commands.is_empty() {
        return Judged { run: Ok(()), accepted: Accepted::Unjudged, output: String::new() };
    }
    if !host.is_empty() && (!config.workflow.is_empty() || commands.iter().any(|command| config.local_reason(command).is_some())) {
        return Judged::unjudged(format!("a check before queueing would run on this host rather than on {host}, so the run does not start"));
    }
    say(&format!("safe-push: running {} check(s) before queueing the lane: {}", commands.len(), commands.join("; ")));
    let outcome = command.and_then(|_| tempfile::NamedTempFile::new().ok());
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), one_lane: true, runner_host: host.to_owned(), outcome: outcome.as_ref().map(|file| file.path().to_owned()), ..Arguments::default() };
    let (status, passed) = match attempts(root, &arguments, &config, &commands, command) {
        Ok(ran) => ran,
        Err(text) => return Judged::unjudged(text),
    };
    let (own, others) = command.zip(outcome.as_ref()).map(|(command, outcome)| failed_output(outcome.path(), command)).unwrap_or_default();
    let run = if status == 0 { Ok(()) } else { Err(refusal(status, clippy.is_some(), (root, base, head))) };
    Judged { accepted: accepted(&run, passed, own.is_some()), output: own.unwrap_or(others), run }
}

type Listed = (Config, Option<String>, Vec<String>);

fn listed(root: &Path, base: &str, head: &str) -> Result<Listed, String> {
    let config = Config::load(root, head);
    let clippy = clippy_first::listed(&config.after_set().commands(root, base, head)?, |command| !config.is_inferred(command));
    Ok((config, clippy, queue_commands(root, base, head)?))
}

/// The run's exit status and whether `watched` passed in it, running it again on another host when one reached no verdict, up to `ATTEMPTS` times; why it cannot run when no build host answers.
fn attempts(root: &Path, arguments: &Arguments, config: &Config, commands: &[String], watched: Option<&str>) -> Result<(i32, Option<bool>), String> {
    let mut failed: Vec<String> = Vec::new();
    let (mut status, mut passed) = (NO_VERDICT_STATUS, None);
    let mut attempt = 1;
    while attempt <= ATTEMPTS {
        let ran = block_on(run_added_avoiding(root.to_owned(), arguments.clone(), config.clone(), commands.to_vec(), watched, &failed)).map_err(|error| error.to_string())?;
        (status, passed) = (ran.status, ran.accepted);
        if ran.unanswered {
            return Err("no build host answers over ssh, so the checks before queueing cannot run; the lane is not queued uncompiled, so run safe-push again once a build host is back".to_owned());
        }
        if status == SLOTS_BUSY_STATUS {
            say("safe-push: every slot on the build host stayed busy for its wait limit; the checks before queueing wait in line again");
            continue;
        }
        if status != NO_VERDICT_STATUS {
            break;
        }
        if attempt < ATTEMPTS {
            warn(&format!("safe-push: the checks before queueing reached no verdict on {}; running them again ({} of {ATTEMPTS})", if ran.host.is_empty() { "this host" } else { ran.host.as_str() }, attempt + 1));
        }
        if !ran.host.is_empty() {
            failed.push(ran.host);
        }
        attempt += 1;
    }
    Ok((status, passed))
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

fn refusal(status: i32, clippy: bool, (root, base, head): (&Path, &str, &str)) -> QueueRefusal {
    if status == NO_VERDICT_STATUS {
        return QueueRefusal::Unjudged(format!("the checks before queueing reached no verdict {ATTEMPTS} times (exit 75: the host or its connection failed before they reported); the lane is not queued uncompiled, so run safe-push again"));
    }
    let rerun = if clippy {
        let crates = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.changed_arguments());
        format!("; to rerun clippy alone: cargo clippy --all-targets {crates} -- -D warnings")
    } else {
        String::new()
    };
    QueueRefusal::Failed(format!("a check before queueing exited {status}; fix the failure above{rerun}"))
}

fn lane_tests(landing: &[String]) -> Option<String> {
    landing.iter().find(|command| verdict_key::reads_it(std::slice::from_ref(command))).cloned()
}
