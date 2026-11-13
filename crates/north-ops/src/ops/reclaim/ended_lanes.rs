use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::fleet::records::{RecordedSession, recorded_sessions};
use crate::platform::land_queue;

use super::held::Held;
use super::scratch::fleet_root;
use super::{Inputs, Policy, Removal, removed, size};

const BUILD_OUTPUT: [&str; 2] = ["target", "buck-out"];
const TICKET_AREAS: [&str; 5] = ["", "outbox", "done", "withdrawn", "outbox/decided"];

struct Session {
    name: String,
    cwd: PathBuf,
    ended: bool,
}

struct Ticket {
    root: PathBuf,
    session: String,
}

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> Vec<Removal> {
    let sessions: Vec<Session> = recorded_sessions(&fleet_root(&inputs.state)).into_iter().map(|RecordedSession { name, cwd, ended, .. }| Session { cwd: canonical(&cwd), name, ended }).collect();
    let queues = land_queue::queues(&inputs.state);
    let tickets = tickets(&queues, &inputs.host);
    let mut removals = Vec::new();
    for lane in lanes(&inputs.code) {
        if held.within(&lane) || !land_queue::waiting_on(&queues, &inputs.host, &lane).is_empty() {
            continue;
        }
        if let Some(ended) = ended_sessions(&lane, &sessions, &tickets) {
            removals.extend(BUILD_OUTPUT.iter().filter_map(|name| build_output(&lane, name, &ended, policy.dry_run)));
        }
    }
    removals
}

fn build_output(lane: &Path, name: &str, ended: &str, dry_run: bool) -> Option<Removal> {
    let path = lane.join(name);
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) || !ignored(lane, name) {
        return None;
    }
    let bytes = size(&path);
    if !dry_run && !removed(&path) {
        return None;
    }
    Some(Removal { path, reason: format!("build output of a lane whose every session has ended ({ended}), with no open hand-in and held by no process; the next build restores it"), bytes })
}

fn ended_sessions(lane: &Path, sessions: &[Session], tickets: &[Ticket]) -> Option<String> {
    let lane_name = lane.file_name()?.to_str()?;
    let mut names: BTreeSet<&str> = sessions.iter().filter(|session| session.cwd.starts_with(lane) || session.name == lane_name).map(|session| session.name.as_str()).collect();
    names.extend(tickets.iter().filter(|ticket| ticket.root == lane).map(|ticket| ticket.session.as_str()));
    let ended = |name: &&str| {
        let mut records = sessions.iter().filter(|session| session.name == *name).peekable();
        records.peek().is_some() && records.all(|session| session.ended)
    };
    (!names.is_empty() && names.iter().all(ended)).then(|| names.into_iter().collect::<Vec<_>>().join(", "))
}

fn tickets(queues: &Path, host: &str) -> Vec<Ticket> {
    let ticket = |path: PathBuf| {
        let ticket: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        ticket["host"].as_str().is_none_or(|named| named == host).then_some(())?;
        let session = ticket["session"].as_str().filter(|session| !session.is_empty())?.to_owned();
        Some(Ticket { root: canonical(Path::new(ticket["root"].as_str()?)), session })
    };
    fs::read_dir(queues)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|queue| TICKET_AREAS.map(|area| queue.path().join(area)))
        .flat_map(|area| fs::read_dir(area).into_iter().flatten().flatten().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(ticket)
        .collect()
}

fn lanes(code: &Path) -> Vec<PathBuf> {
    let directories = |path: PathBuf| fs::read_dir(path).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path());
    let mut lanes: Vec<PathBuf> = directories(code.to_path_buf()).flat_map(|container| directories(container.join("worktrees"))).map(|lane| canonical(&lane)).collect();
    lanes.sort();
    lanes
}

fn ignored(lane: &Path, name: &str) -> bool {
    Command::new("git").arg("-C").arg(lane).args(["check-ignore", "-q", &format!("{name}/")]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
