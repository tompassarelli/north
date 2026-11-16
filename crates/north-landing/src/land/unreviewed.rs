//! A ticket held in the landing queue for the reviewer accept its lane needs (north#1632): the hand-in queues it at once with the patch id the accept is keyed by, and the lander keeps it out of every candidate until it can take it. Each pass lists the accept store's file names once for every held ticket and reads nothing more while no accept names a held patch; once one does, the review-accept and safe-push-keys scans run whole on the head the lander lands over main as it stands, and the ticket joins the next batch only when they pass, or is held again under its new patch id when that head's patch changed. A review call's refusal of the patch with no accept refuses the ticket, its findings in the refusal its author is told; a ticket that waits past its notice is named to the coordinator once, with its author and its age, and stays held.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use north_base::platform::records_mirror::Records;
use north_base::platform::review_accept::{Accepts, is_patch_id, patch_id};
use north_land_scans::review::{Review, review};

use crate::land::after;
use crate::land::bisect;
use crate::land::candidate;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket, Unreviewed};
use crate::land::records::{self, Candidate, Decision};
use crate::land::review_calls::ReviewCalls;
use crate::land::telemetry::Stages;

pub type Lanes = Vec<(PathBuf, Ticket)>;

/// A guess: the review calls' first-commit-to-accept took 73 minutes at the 90th percentile over 2026-11-15 to 16, so two hours names only a review nobody is giving; the ages at which held tickets get their accept would settle it.
const NOTICE: Duration = Duration::from_secs(2 * 3600);

/// The tickets the lander may plan this pass: every ticket not held for a review, and each held one whose accept the scans now count.
pub fn settle(lander: &Lander, tickets: Lanes, now: Duration) -> Result<Lanes, String> {
    if tickets.iter().all(|(_, ticket)| ticket.unreviewed.is_none()) {
        return Ok(tickets);
    }
    let here = Accepts::here();
    let calls = ReviewCalls::of(here.0.parent().unwrap_or(&here.0));
    let accepted = patches(&Accepts::counted(&Records::here()));
    let mut settled = Lanes::new();
    for (path, ticket) in tickets {
        match ticket.unreviewed.clone() {
            None => settled.push((path, ticket)),
            Some(held) => settled.extend(held_ticket(lander, (path, ticket), held, (&accepted, &calls), now)?),
        }
    }
    Ok(settled)
}

fn held_ticket(lander: &Lander, (path, ticket): (PathBuf, Ticket), held: Unreviewed, (accepted, calls): (&BTreeSet<String>, &ReviewCalls), now: Duration) -> Result<Option<(PathBuf, Ticket)>, String> {
    if accepted.contains(&held.patch) {
        return taken(lander, path, ticket, &held);
    }
    if let Some(findings) = calls.refused(&held.patch) {
        refuse(lander, &path, &ticket, &findings, now)?;
    } else if !held.noted && age(&ticket, now) >= NOTICE {
        note(&path, ticket, held, now)?;
    }
    Ok(None)
}

fn taken(lander: &Lander, path: PathBuf, ticket: Ticket, held: &Unreviewed) -> Result<Option<(PathBuf, Ticket)>, String> {
    let Ok(base) = candidate::prepare(&lander.repo, &lander.worktree) else { return Ok(None) };
    if !candidate::readable(&lander.worktree, &ticket) {
        return Ok(None);
    }
    let fork = candidate::git(&lander.worktree, &["merge-base", &base, &ticket.head])?;
    let patch = patch_id(&lander.worktree, &fork, &ticket.head)?;
    if patch != held.patch {
        println!("north land: {} changed its patch from {} to {patch} while held, so it waits for an accept of {patch}", ticket.branch, held.patch);
        write(path, Ticket { unreviewed: Some(Unreviewed { patch, noted: false }), ..ticket })?;
        return Ok(None);
    }
    if review(&lander.worktree, &base, &ticket.head) != Review::Passed {
        return Ok(None);
    }
    println!("north land: {} holds the review accept of patch {patch}; it joins the next batch", ticket.branch);
    write(path, Ticket { unreviewed: None, ..ticket }).map(Some)
}

fn refuse(lander: &Lander, path: &Path, ticket: &Ticket, findings: &str, now: Duration) -> Result<(), String> {
    let logs = lander.queue.join("logs");
    let log = logs.join(format!("{}-{}-review.log", now.as_micros(), ticket.branch));
    std::fs::create_dir_all(&logs).and_then(|()| std::fs::write(&log, format!("{findings}\n"))).map_err(|error| format!("cannot write {}: {error}", log.display()))?;
    let refusal = format!("failed {}: the review call refused the lane's patch; a changed patch handed in again is reviewed again", log.display());
    records::write(lander, path, ticket, &Candidate { head: &ticket.head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &refusal });
    let main = candidate::prepare(&lander.repo, &lander.worktree)?;
    bisect::decide(path, ticket, refusal, Some(&log), &main)
}

fn note(path: &Path, ticket: Ticket, held: Unreviewed, now: Duration) -> Result<(), String> {
    let author = ticket.session.as_deref().unwrap_or("a session north does not know");
    let message = format!("north land: {} by {author} has waited {} minutes in the landing queue for a review accept of patch {}, and no review verdict is recorded; it stays held", ticket.branch, age(&ticket, now).as_secs() / 60, held.patch);
    println!("{message}");
    if !after::send("coordinator", &message) {
        println!("north land: could not tell the coordinator that {} waits for its review", ticket.branch);
    }
    write(path.to_owned(), Ticket { unreviewed: Some(Unreviewed { noted: true, ..held }), ..ticket }).map(drop)
}

fn write(path: PathBuf, ticket: Ticket) -> Result<(PathBuf, Ticket), String> {
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok((path, ticket))
}

fn age(ticket: &Ticket, now: Duration) -> Duration {
    let queued = ticket.queued_micros.unwrap_or(now.as_micros());
    Duration::from_micros(u64::try_from(now.as_micros().saturating_sub(queued)).unwrap_or(u64::MAX))
}

/// The patch ids that hold any accept, read from the store's file names, `PATCH.SESSION.json`, alone: one listing and no file read.
fn patches(accepts: &Accepts) -> BTreeSet<String> {
    let names = std::fs::read_dir(&accepts.0).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok());
    names.filter_map(|name| name.split_once('.').map(|(patch, _)| patch.to_owned())).filter(|patch| is_patch_id(patch)).collect()
}
