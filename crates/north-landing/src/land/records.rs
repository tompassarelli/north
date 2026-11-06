use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::platform::push_ledger;
use super::after::{cleanup, follow};
use super::candidate::git;
use super::incidents;
use super::lander::Lander;
use super::queue::{self, Ticket};
use super::telemetry::{self, Stages};

pub struct Candidate<'a> {
    pub head: &'a str,
    pub batch_size: usize,
    pub stages: &'a Stages,
}

pub enum Decision<'a> {
    Landed { sha: &'a str },
    Refused { verdict: &'a str },
}

fn class(verdict: &str) -> Option<&'static str> {
    match verdict.split_whitespace().next() {
        Some("failed") => Some("failed"),
        Some("conflict") => Some("conflict"),
        _ => None,
    }
}

fn flag(arguments: &mut Vec<String>, name: &str, value: impl ToString) {
    arguments.push(name.to_owned());
    arguments.push(value.to_string());
}

pub fn arguments(path: &Path, ticket: &Ticket, candidate: &Candidate, tree: &str, decision: &Decision, at: f64) -> Option<Vec<String>> {
    let queued = queue::queued_micros(path);
    if queued == u128::MAX {
        return None;
    }
    let lane = ticket.root.file_name()?.to_string_lossy().into_owned();
    let mut arguments = vec!["land-train".to_owned(), "record".to_owned()];
    flag(&mut arguments, "--repo", push_ledger::repository(&ticket.origin));
    flag(&mut arguments, "--lane", lane);
    flag(&mut arguments, "--branch", &ticket.branch);
    flag(&mut arguments, "--tip", &ticket.head);
    flag(&mut arguments, "--queued-at", queued as f64 / 1e6);
    if let Some(first) = ticket.first_commit_secs {
        flag(&mut arguments, "--first-commit-at", first);
    }
    if !candidate.head.is_empty() {
        flag(&mut arguments, "--candidate", candidate.head);
        flag(&mut arguments, "--tree", tree);
    }
    flag(&mut arguments, "--batch-size", candidate.batch_size);
    flag(&mut arguments, "--bisect-depth", candidate.stages.bisect_depth);
    flag(&mut arguments, "--attempts", candidate.stages.attempts);
    if let Some((started, finished)) = candidate.stages.check_window() {
        flag(&mut arguments, "--check-started-at", started);
        flag(&mut arguments, "--check-finished-at", finished);
    }
    match decision {
        Decision::Landed { sha } => {
            flag(&mut arguments, "--verdict", "landed");
            flag(&mut arguments, "--landed-at", at);
            flag(&mut arguments, "--landed-sha", sha);
        }
        Decision::Refused { verdict } => {
            flag(&mut arguments, "--verdict", class(verdict)?);
            flag(&mut arguments, "--detail", verdict);
        }
    }
    Some(arguments)
}

pub(super) fn spawn(queue: &Path, arguments: &[String]) -> Result<(), String> {
    let logs = queue.join("logs");
    fs::create_dir_all(&logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = OpenOptions::new().create(true).append(true).open(logs.join("landing-records.log")).map_err(|error| error.to_string())?;
    let errors = log.try_clone().map_err(|error| error.to_string())?;
    let mut child = Command::new(follow::program())
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .spawn()
        .map_err(|error| format!("{} {}: {error}", follow::program(), arguments.get(..2).unwrap_or(arguments).join(" ")))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn write(lander: &Lander, path: &Path, ticket: &Ticket, candidate: &Candidate, decision: &Decision) {
    let tree = if candidate.head.is_empty() { String::new() } else { git(&lander.worktree, &["rev-parse", &format!("{}^{{tree}}", candidate.head)]).unwrap_or_default() };
    if let Decision::Refused { verdict } = decision {
        incidents::record(&lander.queue, path, ticket, incidents::decided(path, ticket, verdict));
    }
    let landed = match decision {
        Decision::Landed { sha } => Some(*sha),
        Decision::Refused { verdict } => verdict.strip_prefix("landed ").and_then(|rest| rest.split_whitespace().next()),
    };
    if let Some(landed) = landed {
        cleanup::origin_branches(lander, ticket, landed);
    }
    let Some(arguments) = arguments(path, ticket, candidate, &tree, decision, telemetry::epoch()) else { return };
    if let Err(error) = spawn(&lander.queue, &arguments) {
        println!("north land: could not record {}: {error}", ticket.branch);
    }
}
