pub mod cleanup;
pub mod follow;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::candidate::git;
use super::queue::{self, Ticket};
use super::lander::{Lander, state_home};
use super::records::{self, Candidate, Decision};
use super::telemetry::{self, Push, Stages};

const SEND_WAIT: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
struct Intent {
    head: String,
    base: String,
    tickets: Vec<String>,
}

fn area(lander: &Lander) -> PathBuf {
    lander.queue.join("landing")
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn begin(lander: &Lander, head: &str, base: &str, tickets: &[(&PathBuf, &Ticket)]) -> Result<(), String> {
    let intent = Intent { head: head.to_owned(), base: base.to_owned(), tickets: tickets.iter().map(|(path, _)| name_of(path)).collect() };
    let path = area(lander).join("intent.json");
    fs::create_dir_all(area(lander)).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_string(&intent).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

pub fn abandon(lander: &Lander) {
    let _ = fs::remove_file(area(lander).join("intent.json"));
}

pub fn send(session: &str, message: &str) -> bool {
    let Ok(mut child) = Command::new(follow::program()).args(["fleet", "send", session, message]).stdin(Stdio::null()).spawn() else {
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
    let on_main = git(&lander.worktree, &["cherry", "FETCH_HEAD", &ticket.head]).is_ok_and(|cherry| !cherry.lines().any(|line| line.starts_with('+')));
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
    let follow = follow::Follow { repo: &lander.repo, git_dir: Path::new(&git_dir), base, head, safe_push: &follow::executable(lander.checker.first().map_or("safe-push", String::as_str)), log: &log };
    if let Err(error) = follow::spawn(&follow) {
        println!("north land: {error}");
    }
}

pub fn finish(lander: &Lander, head: &str, base: &str, stages: &Stages, tickets: Vec<(PathBuf, Ticket)>) -> Result<(), String> {
    let _ = git(&lander.worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"]);
    let state = state_home();
    let batch_size = tickets.len();
    for (path, ticket) in tickets {
        let name = name_of(&path);
        let repository = telemetry::repository(&ticket.origin);
        let push = Push { at_secs: telemetry::seconds(), repository: &repository, branch: &ticket.branch, oid: &ticket.head, landed: head, session: ticket.session.as_deref().unwrap_or("") };
        if let Err(error) = telemetry::record_push(&state, &push) {
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
    follow_once(lander, base, head);
    abandon(lander);
    Ok(())
}

pub fn recover(lander: &Lander) -> Result<(), String> {
    let path = area(lander).join("intent.json");
    let Ok(text) = fs::read_to_string(&path) else { return Ok(()) };
    let Ok(intent) = serde_json::from_str::<Intent>(&text) else {
        abandon(lander);
        return Ok(());
    };
    git(&lander.worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    let landed = super::candidate::output(&lander.worktree, &["merge-base", "--is-ancestor", &intent.head, "FETCH_HEAD"]).is_ok_and(|output| output.status.success());
    if !landed {
        abandon(lander);
        return Ok(());
    }
    let tickets = intent.tickets.iter().filter_map(|name| {
        let path = lander.queue.join(name);
        queue::read(&path).ok().map(|ticket| (path, ticket))
    });
    println!("north land: finishing the interrupted landing of candidate {}", intent.head);
    finish(lander, &intent.head, &intent.base, &Stages::empty(), tickets.collect())
}
