use std::fs;
use std::path::Path;

use crate::landing::check_findings;
use crate::platform::attribution::Cause;
pub use crate::platform::land_notice::OWNER;
use crate::platform::land_notice::{self, Notice};
use super::after;
use super::checks::Verdict;
use super::incidents;
use super::queue::{self, Ticket};
use super::refusal;

#[derive(Debug, PartialEq, Eq)]
pub enum Judgement {
    Land,
    Halve(usize),
    Culprit(String),
    Rebuild,
    RedMain(String),
    Hold(String),
    Paused(String),
}

pub fn half(included: usize) -> usize {
    included.div_ceil(2)
}

pub fn judge(verdict: Verdict, included: usize, base_moved: bool) -> Judgement {
    match verdict {
        Verdict::Green => Judgement::Land,
        _ if base_moved => Judgement::Rebuild,
        Verdict::Refused(_) if included > 1 => Judgement::Halve(half(included)),
        Verdict::Refused(detail) => Judgement::Culprit(detail),
        Verdict::RedMain(detail) => Judgement::RedMain(detail),
        Verdict::Transient(detail) => Judgement::Hold(format!("transient {detail}")),
        Verdict::BrokenBase(detail) => Judgement::Paused(detail),
        Verdict::Deferred(detail) => Judgement::Hold(format!("deferred {detail}")),
    }
}

fn rejected(verdict: &str) -> bool {
    verdict.starts_with("failed ") || verdict.starts_with("conflict ")
}

/// A refusal by the lane's own failing checks, whose error count dispatch compares across the lane's tries.
fn own_failure(verdict: &str) -> bool {
    verdict.starts_with("failed ") && incidents::blame(verdict) == Some(Cause::Worker)
}

fn refused(ticket: &Ticket, verdict: &str, log: Option<&Path>) -> String {
    let read = log.and_then(|log| fs::read_to_string(log).ok());
    let errors = read.as_deref().filter(|_| own_failure(verdict)).map(check_findings::errors);
    let mut text = Notice::Refused { branch: ticket.branch.clone(), head: ticket.head.clone(), verdict: verdict.to_owned(), errors }.text();
    if let Some(log) = log {
        let reason = refusal::reason(read.as_deref().unwrap_or_default());
        if !reason.is_empty() {
            text.push('\n');
            text.push_str(&reason);
        }
        text.push_str(&format!("\nfull log: {}", log.display()));
    }
    text
}

pub fn notify(ticket: &Ticket, verdict: &str, log: Option<&Path>) {
    let sessions = land_notice::told(ticket.session.as_deref(), verdict);
    let Some(first) = sessions.first() else { return };
    let text = match verdict.strip_prefix("landed ") {
        Some(sha) => Notice::Landed { branch: ticket.branch.clone(), sha: sha.to_owned() }.text(),
        None => refused(ticket, verdict, log),
    };
    if !sessions.iter().any(|session| after::send(session, &text)) {
        eprintln!("north land: could not tell session {first} about {}", ticket.branch);
    }
}

pub fn decide(path: &Path, ticket: &Ticket, verdict: String, log: Option<&Path>, main: &str) -> Result<(), String> {
    println!("north land: {} ({}): {verdict}", ticket.branch, ticket.head);
    queue::decide(path, Ticket { decided_on: Some(main.to_owned()), ..ticket.clone() }, verdict.clone()).map_err(|error| error.to_string())?;
    if rejected(&verdict) || verdict.starts_with("landed ") {
        notify(ticket, &verdict, log);
    }
    Ok(())
}

