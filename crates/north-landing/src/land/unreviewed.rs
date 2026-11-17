//! A ticket held in the landing queue for the reviewer accept its lane needs (north#1632): the hand-in queues it at once with the patch id the accept is keyed by, and the lander keeps it out of every candidate until it can take it. Each pass lists the accept store's file names once for every held ticket; a held patch no accept names gets one security read (land/security_read.rs, north#1650), which the lander runs on the head it holds, in a task of its own so no pass waits on it, and records as the accept of that patch id when it accepts. Once an accept names a held patch, the review-accept and safe-push-keys scans run whole on the head the lander lands over main as it stands, and the ticket joins the next batch only when they pass, or is held again under its new patch id when that head's patch changed. A read that refuses or fails refuses the ticket, with its answer in the refusal its author is told, and a refused patch handed in again is refused again with no new read; a ticket that waits past its notice is named to the coordinator once, with its author and its age, and stays held.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use north_base::platform::records_mirror::Records;
use north_base::platform::review_accept::{Accept, Accepts, CALL_SESSION, is_patch_id, patch_id};
use north_land_scans::review::{Review, review};

use crate::land::after;
use crate::land::bisect;
use crate::land::candidate;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket, Unreviewed};
use crate::land::records::{self, Decision};
use crate::land::security_read::{self, Kept, Verdict};
use crate::land::wake::Task;

pub type Lanes = Vec<(PathBuf, Ticket)>;

/// A guess: the review calls' first-commit-to-accept took 73 minutes at the 90th percentile over 2026-11-15 to 16, so two hours names only a review nobody is giving; the ages at which held tickets get their accept would settle it.
const NOTICE: Duration = Duration::from_secs(2 * 3600);

/// The security reads running or finished and not yet read, by the patch id each reads.
#[derive(Default)]
pub struct Reads(HashMap<String, Task<Verdict>>);

impl Reads {
    pub fn unread(&self) -> usize {
        self.0.len()
    }
}

/// The accepts a landing counts and the verdicts the security reads gave.
struct Stores {
    accepts: Accepts,
    accepted: BTreeSet<String>,
    kept: Kept,
}

/// The tickets the lander may plan this pass: every ticket not held for a review, and each held one whose accept the scans now count.
pub fn settle(lander: &Lander, reads: &mut Reads, tickets: Lanes, now: Duration) -> Result<Lanes, String> {
    reads.0.retain(|patch, _| tickets.iter().any(|(_, ticket)| ticket.unreviewed.as_ref().is_some_and(|held| held.patch == *patch)));
    if tickets.iter().all(|(_, ticket)| ticket.unreviewed.is_none()) {
        return Ok(tickets);
    }
    let accepts = Accepts::counted(&Records::here());
    let here = Accepts::here();
    let stores = Stores { accepted: patches(&accepts), kept: Kept::of(here.0.parent().unwrap_or(&here.0)), accepts };
    let mut settled = Lanes::new();
    for (path, ticket) in tickets {
        match ticket.unreviewed.clone() {
            None => settled.push((path, ticket)),
            Some(held) => settled.extend(held_ticket(lander, reads, (path, ticket), held, &stores, now)?),
        }
    }
    Ok(settled)
}

fn held_ticket(lander: &Lander, reads: &mut Reads, (path, ticket): (PathBuf, Ticket), held: Unreviewed, stores: &Stores, now: Duration) -> Result<Option<(PathBuf, Ticket)>, String> {
    if stores.accepted.contains(&held.patch) {
        return taken(lander, path, ticket, &held);
    }
    if let Some(answer) = stores.kept.refused(&held.patch) {
        refuse(lander, &path, &ticket, &held.patch, &answer, now)?;
        return Ok(None);
    }
    match reads.0.remove(&held.patch) {
        Some(read) if !read.is_finished() => {
            reads.0.insert(held.patch.clone(), read);
        }
        Some(read) => {
            let verdict = read.join().unwrap_or_else(|_| Verdict::Failed(format!("the security read of patch {} panicked", held.patch)));
            return read_done(lander, (path, ticket), &held, stores, verdict, now);
        }
        None => {
            if let Some((base, fork)) = current(lander, &path, &ticket, &held)? {
                println!("north land: {} needs a review accept of patch {}; the lander makes one security read of it", ticket.branch, held.patch);
                let (worktree, head, patch) = (lander.worktree.clone(), ticket.head.clone(), held.patch.clone());
                let read = Task::spawn(&lander.wake, move || security_read::read(&worktree, (&base, &fork, &head), &patch));
                reads.0.insert(held.patch.clone(), read);
            }
        }
    }
    if !held.noted && age(&ticket, now) >= NOTICE {
        note(&path, ticket, held, now)?;
    }
    Ok(None)
}

fn read_done(lander: &Lander, (path, ticket): (PathBuf, Ticket), held: &Unreviewed, stores: &Stores, verdict: Verdict, now: Duration) -> Result<Option<(PathBuf, Ticket)>, String> {
    stores.kept.keep(&held.patch, &verdict)?;
    match verdict {
        Verdict::Accept(answer) => {
            let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
            stores.accepts.record_call(&Accept { patch_id: held.patch.clone(), session: CALL_SESSION.to_owned(), at })?;
            println!("north land: the security read accepts patch {} of {}:\n{answer}", held.patch, ticket.branch);
            taken(lander, path, ticket, held)
        }
        Verdict::Refuse(answer) | Verdict::Failed(answer) => {
            refuse(lander, &path, &ticket, &held.patch, &answer, now)?;
            Ok(None)
        }
    }
}

/// Main as the lander prepared it and the lane's fork from it, when the held head is readable and still makes the held patch; a head whose patch changed is held again under its new patch id.
fn current(lander: &Lander, path: &Path, ticket: &Ticket, held: &Unreviewed) -> Result<Option<(String, String)>, String> {
    let Ok(base) = candidate::prepare(&lander.repo, &lander.worktree) else { return Ok(None) };
    if !candidate::readable(&lander.worktree, ticket) {
        return Ok(None);
    }
    let fork = candidate::git(&lander.worktree, &["merge-base", &base, &ticket.head])?;
    let patch = patch_id(&lander.worktree, &fork, &ticket.head)?;
    if patch != held.patch {
        println!("north land: {} changed its patch from {} to {patch} while held, so it waits for an accept of {patch}", ticket.branch, held.patch);
        write(path.to_owned(), Ticket { unreviewed: Some(Unreviewed { patch, noted: false }), ..ticket.clone() })?;
        return Ok(None);
    }
    Ok(Some((base, fork)))
}

fn taken(lander: &Lander, path: PathBuf, ticket: Ticket, held: &Unreviewed) -> Result<Option<(PathBuf, Ticket)>, String> {
    let Some((base, _)) = current(lander, &path, &ticket, held)? else { return Ok(None) };
    if review(&lander.worktree, &base, &ticket.head) != Review::Passed {
        return Ok(None);
    }
    println!("north land: {} holds the review accept of patch {}; it joins the next batch", ticket.branch, held.patch);
    write(path, Ticket { unreviewed: None, ..ticket }).map(Some)
}

fn refuse(lander: &Lander, path: &Path, ticket: &Ticket, patch: &str, answer: &str, now: Duration) -> Result<(), String> {
    let logs = lander.queue.join("logs");
    let log = logs.join(format!("{}-{}-review.log", now.as_micros(), ticket.branch));
    std::fs::create_dir_all(&logs).and_then(|()| std::fs::write(&log, format!("{answer}\n"))).map_err(|error| format!("cannot write {}: {error}", log.display()))?;
    let refusal = format!("failed {}: the security read refused the lane's patch {patch}; a changed patch handed in again is read again", log.display());
    records::settle(lander, ticket, &Decision::Refused { verdict: &refusal });
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
