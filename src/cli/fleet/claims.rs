//! The `north threads` adapter behind the fleet's claims: a session holds its task in the ledger dispatch reads.
use std::fs::{File, OpenOptions, TryLockError};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::platform::process;
use crate::fleet::claims::{Claims, Rows};
use crate::fleet::registry::Fleet;
use crate::threads::catalog::CATALOG;
use crate::threads::claims::verbs::OWNED;

/// The `repo#N` form `threads claim` records, whichever spelling `--task` carried.
fn canonical(item: &str) -> String {
    CATALOG.parse_ref(item).map_or_else(|_| item.to_owned(), |(repo, number)| format!("{repo}#{number}"))
}

/// A host outlives runtime updates, and Linux then reports its replaced binary with a " (deleted)" suffix.
fn running_north() -> NorthResult<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.to_str().and_then(|path| path.strip_suffix(" (deleted)")).map_or(exe.clone(), std::path::PathBuf::from))
}

/// The ledger's line when the claim moved to `session` from an earlier owner; a fresh or renewed claim says only that `session` owns the item.
fn moved(said: &str, item: &str, session: &str) -> Option<String> {
    (said != format!("{session} owns {}", canonical(item))).then(|| said.to_owned())
}

/// A verb that only reads leaves the sweep to the mutating verbs, because the hooks that call `fleet list` run on every prompt.
const READ_VERBS: [&str; 6] = ["list", "stranded", "tree", "spend", "accounts", "show"];

/// A launch asks the ledger only about the items it names, through [`Fleet::release_exited_claims_on`], so parallel launches never each read every claim.
const SCOPED_VERBS: [&str; 1] = ["launch"];

/// The verb a mutating verb starts in the background to clear exited sessions' claims.
pub(super) const SWEEP_VERB: &str = "sweep-claims";

/// Whether `arguments` name a verb that starts the background claims sweep.
fn sweeps(arguments: &[String]) -> bool {
    !arguments.first().is_some_and(|verb| READ_VERBS.contains(&verb.as_str()) || SCOPED_VERBS.contains(&verb.as_str()) || verb == SWEEP_VERB)
}

/// Starts the claims sweep in the background when a mutating verb finds exited sessions no sweep has cleared, so no verb waits on the threads ledger, which sits across ssh on a host that can be loaded, or reports its slowness.
pub(super) fn release_exited_claims(fleet: &Fleet, arguments: &[String]) {
    if !sweeps(arguments) {
        return;
    }
    let started = fleet.unswept().and_then(|unswept| if unswept.is_empty() { Ok(()) } else { start_sweep(fleet) });
    if let Err(error) = started {
        eprintln!("north fleet: exited sessions' claims sweep not started: {}", error.user_message());
    }
}

fn start_sweep(fleet: &Fleet) -> NorthResult<()> {
    let Some(log) = sweep_log_unless_running(fleet)? else {
        return Ok(());
    };
    log.unlock()?;
    let mut command = Command::new(running_north()?);
    command.args(["fleet", SWEEP_VERB]).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log).process_group(0);
    let mut child = command.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Clears every exited session's claims, one sweep per fleet at a time: a sweep that finds another holding the log leaves the backlog to it, and the one that holds it writes only its own outcome there.
pub(super) fn sweep(fleet: &Fleet) -> NorthResult<()> {
    let Some(log) = sweep_log_unless_running(fleet)? else {
        return Ok(());
    };
    log.set_len(0)?;
    println!("released {} claims exited sessions held", fleet.release_exited_claims()?);
    Ok(())
}

/// The sweep log, opened for appending and locked by this process, or `None` while a running sweep holds it.
fn sweep_log_unless_running(fleet: &Fleet) -> NorthResult<Option<File>> {
    let log = OpenOptions::new().create(true).append(true).open(fleet.claims_sweep_log())?;
    match log.try_lock() {
        Ok(()) => Ok(Some(log)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

/// How long a fleet verb waits on one threads ledger call, because the ledger can sit across ssh behind a hung courier and a verb must not wait on it.
const LEDGER_LIMIT: Duration = Duration::from_secs(2);

/// How long the background claims sweep waits on one threads ledger call: no verb waits on it, so it outlasts the threads client's own 20 s ssh bound.
const SWEEP_LIMIT: Duration = Duration::from_secs(30);

pub struct ThreadsCli {
    program: Option<PathBuf>,
    limit: Duration,
}

impl ThreadsCli {
    pub fn new() -> Self {
        Self { program: None, limit: LEDGER_LIMIT }
    }

    /// The ledger adapter for the verb `arguments` name: the background claims sweep waits longer on each call than a verb someone is waiting on.
    pub fn for_verb(arguments: &[String]) -> Self {
        let limit = if arguments.first().is_some_and(|verb| verb == SWEEP_VERB) { SWEEP_LIMIT } else { LEDGER_LIMIT };
        Self { program: None, limit }
    }

    fn threads(&self, args: &[&str]) -> NorthResult<String> {
        let program = match &self.program {
            Some(program) => program.clone(),
            None => running_north()?,
        };
        let mut command = Command::new(program);
        command.arg("threads").args(args);
        let Some(output) = process::run_group_within(command, self.limit) else {
            return Err(NorthError::State(format!("the threads ledger did not answer `threads {}` within {} s", args.join(" "), self.limit.as_secs())));
        };
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(NorthError::Refused(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
    }
}

impl Claims for ThreadsCli {
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
        match self.threads(&["claim", item, "--by", session]) {
            Err(NorthError::Refused(why)) if !why.contains(OWNED) => Err(NorthError::State(why)),
            other => other.map(|said| moved(said.trim(), item, session)),
        }
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        self.threads(&["release", &canonical(item), "--by", session, "--held-by", session]).map(drop)
    }

    fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
        let words: Vec<String> = match rows {
            Rows::Items(items) => items.iter().map(|item| canonical(item)).collect(),
            Rows::Owners(owners) => owners.iter().flat_map(|owner| ["--owner".to_owned(), owner.clone()]).collect(),
        };
        let args: Vec<&str> = std::iter::once("holder").chain(words.iter().map(String::as_str)).collect();
        let rows: Value = serde_json::from_str(&self.threads(&args)?)?;
        Ok(rows.as_array().into_iter().flatten().filter_map(|row| Some((row["item"].as_str()?.to_owned(), row["owner"].as_str()?.to_owned()))).collect())
    }
}
