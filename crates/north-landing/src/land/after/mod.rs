pub mod cleanup;
mod ci_budget;
pub mod follow;
pub mod gates;
pub mod goals;
pub mod owner;
pub mod tick;
mod workflow_paths;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::platform::push_ledger;
use super::candidate::git;
use super::queue::{self, Ticket};
use super::lander::{Lander, state_home};
use super::records::{self, Candidate, Decision};
use super::telemetry::{self, Stages};
use super::watch::LandedLane;
use super::trace;

const SEND_WAIT: Duration = Duration::from_secs(60);
const INTENT: &str = "intent.json";
const RECOVERING: &str = "recovering.json";
const LANDED_MAIN: &str = "refs/lander/landed-main";

fn fetch_main(lander: &Lander) -> Result<String, String> {
    git(&lander.worktree, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", "origin", &format!("+refs/heads/main:{LANDED_MAIN}")])
}

#[derive(Serialize, Deserialize)]
struct Intent {
    head: String,
    base: String,
    tickets: Vec<String>,
    #[serde(default)]
    tips: Vec<String>,
}

fn area(lander: &Lander) -> PathBuf {
    lander.queue.join("landing")
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn begin(lander: &Lander, head: &str, base: &str, tickets: &[(&PathBuf, &Ticket)], tips: &[String]) -> Result<(), String> {
    let intent = Intent { head: head.to_owned(), base: base.to_owned(), tickets: tickets.iter().map(|(path, _)| name_of(path)).collect(), tips: tips.to_vec() };
    let path = intent_path(lander);
    fs::create_dir_all(area(lander)).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_string(&intent).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

pub fn intent_path(lander: &Lander) -> PathBuf {
    area(lander).join(INTENT)
}

pub fn abandon(lander: &Lander) {
    let _ = fs::remove_file(intent_path(lander));
}

pub fn claim(lander: &Lander) -> Option<PathBuf> {
    let claim = area(lander).join(RECOVERING);
    if claim.exists() || fs::rename(intent_path(lander), &claim).is_ok() { Some(claim) } else { None }
}

pub fn recovering(lander: &Lander) -> Vec<String> {
    fs::read_to_string(area(lander).join(RECOVERING)).ok().and_then(|text| serde_json::from_str::<Intent>(&text).ok()).map(|intent| intent.tickets).unwrap_or_default()
}

pub fn send(session: &str, message: &str) -> bool {
    let Ok(mut child) = Command::new(follow::program()).args(["fleet", "send", "--from", "service:north-land", session, message]).stdin(Stdio::null()).env_remove("NORTH_RUNTIME_LINK").spawn() else {
        return false;
    };
    let deadline = Instant::now() + SEND_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn notify(lander: &Lander, name: &str, ticket: &Ticket, head: &str) {
    let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) else { return };
    let marker = area(lander).join("notified").join(name);
    if marker.exists() {
        return;
    }
    let on_main = git(&lander.worktree, &["cherry", LANDED_MAIN, &ticket.head]).is_ok_and(|cherry| !cherry.lines().any(|line| line.starts_with('+')));
    if !on_main {
        println!("north land: {} ({}) is not on main yet; no landed message sent", ticket.branch, ticket.head);
        return;
    }
    if send(session, &format!("landed {} as {head}", ticket.branch)) {
        if let Some(parent) = marker.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&marker, head);
    } else {
        println!("north land: could not tell session {session} that {} landed", ticket.branch);
    }
}

fn follow_once(lander: &Lander, base: &str, head: &str) {
    let marker = area(lander).join("followed").join(head);
    if let Some(parent) = marker.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if fs::OpenOptions::new().write(true).create_new(true).open(&marker).is_err() {
        return;
    }
    let Ok(git_dir) = git(&lander.repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) else { return };
    let log = lander.queue.join("logs").join(format!("follow-{}.log", &head[..head.len().min(12)]));
    let follow = follow::Follow { repo: &lander.repo, git_dir: Path::new(&git_dir), base, head, log: &log };
    if let Err(error) = follow::spawn(&follow) {
        println!("north land: {error}");
    }
}

pub fn finish(lander: &Lander, claim: &Path, head: &str, base: &str, stages: &Stages, tickets: Vec<(PathBuf, Ticket)>) -> Result<(), String> {
    let _ = fetch_main(lander);
    let state = state_home();
    let batch_size = tickets.len();
    let mut spans = trace::landing::shared(stages, head);
    for (path, ticket) in tickets {
        let name = name_of(&path);
        let queued = crate::platform::land_queue::rfc3339_seconds(&ticket.queued_at).unwrap_or_else(|| stages.started_at());
        let ticket_name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        spans.extend(trace::landing::spans(stages, &trace::landing::Landed { ticket: &ticket_name, branch: &ticket.branch, queued, landed: head, batch: batch_size }));
        let repository = push_ledger::repository(&ticket.origin);
        let push = push_ledger::Push { at_secs: telemetry::seconds(), repository: &repository, branch: &ticket.branch, oid: &ticket.head, landed: head, session: ticket.session.as_deref().unwrap_or("") };
        if let Err(error) = push_ledger::record(&state, &push) {
            println!("north land: could not record the push of {}: {error}", ticket.branch);
        }
        if let Err(error) = stages.flush(&state, &repository, &ticket.branch) {
            println!("north land: could not record stages of {}: {error}", ticket.branch);
        }
        records::write(lander, &path, &ticket, &Candidate { head, batch_size, stages }, &Decision::Landed { sha: head });
        notify(lander, &name, &ticket, head);
        cleanup::report(lander, &ticket, head);
        println!("north land: {} ({}): landed {head}", ticket.branch, ticket.head);
        queue::decide(&path, ticket, format!("landed {head}")).map_err(|error| error.to_string())?;
    }
    if let Err(error) = trace::Spans::beside(&lander.queue.join("logs")).append(&spans) {
        println!("north land: could not record the spans of {head}: {error}");
    }
    follow_once(lander, base, head);
    let _ = fs::remove_file(claim);
    Ok(())
}

pub struct Recovered {
    pub base: String,
    pub head: String,
    pub lanes: Vec<LandedLane>,
}

pub fn recover(lander: &Lander, claim: &Path) -> Result<Option<Recovered>, String> {
    let Ok(text) = fs::read_to_string(claim) else { return Ok(None) };
    let Ok(intent) = serde_json::from_str::<Intent>(&text) else {
        let _ = fs::remove_file(claim);
        return Ok(None);
    };
    fetch_main(lander)?;
    let landed = super::candidate::output(&lander.worktree, &["merge-base", "--is-ancestor", &intent.head, LANDED_MAIN]).is_ok_and(|output| output.status.success());
    if !landed {
        let _ = fs::remove_file(claim);
        return Ok(None);
    }
    let read = |name: &String| {
        let path = lander.queue.join(name);
        queue::read(&path).ok().map(|ticket| (path, ticket))
    };
    let ticket_of = |name: &String| queue::read(&lander.queue.join(name)).or_else(|_| queue::read(&lander.queue.join("done").join(name))).ok();
    let lanes = intent.tickets.iter().zip(&intent.tips).filter_map(|(name, tip)| Some(LandedLane { ticket: ticket_of(name)?, tip: tip.clone() })).collect();
    let tickets = intent.tickets.iter().filter_map(read);
    println!("north land: finishing the interrupted landing of candidate {}", intent.head);
    finish(lander, claim, &intent.head, &intent.base, &Stages::empty(), tickets.collect())?;
    Ok(Some(Recovered { base: intent.base, head: intent.head, lanes }))
}
