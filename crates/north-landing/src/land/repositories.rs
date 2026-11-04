use std::path::{Path, PathBuf};

use super::candidate::git;
use super::queue::{self, Ticket};

const RECENT: usize = 50;

#[derive(Debug, PartialEq, Eq)]
pub enum Mapping {
    Checkout(PathBuf),
    Unmapped(String),
}

pub fn checkout(root: &Path) -> Result<PathBuf, String> {
    let common = PathBuf::from(git(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    match (common.file_name(), common.parent()) {
        (Some(name), Some(checkout)) if name == ".git" => Ok(checkout.to_owned()),
        _ => Err(format!("{} keeps its git directory at {}, not in a .git directory inside a checkout, so a lander cannot give it worktrees", root.display(), common.display())),
    }
}

fn origin_key(checkout: &Path) -> Result<String, String> {
    git(checkout, &["remote", "get-url", "--push", "origin"]).map(|origin| queue::key(&origin))
}

pub fn servable(root: &Path) -> Result<PathBuf, String> {
    let checkout = checkout(root)?;
    origin_key(&checkout)?;
    Ok(checkout)
}

fn recent(queue: &Path, host: &str) -> Vec<(PathBuf, Ticket)> {
    let mut tickets = queue::pending(queue).unwrap_or_default();
    let mut done: Vec<PathBuf> = std::fs::read_dir(queue.join("done")).map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect()).unwrap_or_default();
    done.sort_by(|left, right| queue::queued_micros(right).cmp(&queue::queued_micros(left)).then_with(|| right.cmp(left)));
    done.truncate(RECENT);
    tickets.extend(done.into_iter().filter_map(|path| queue::read(&path).ok().map(|ticket| (path, ticket))));
    tickets.retain(|(_, ticket)| ticket.host == host);
    tickets.sort_by(|(left, _), (right, _)| queue::queued_micros(right).cmp(&queue::queued_micros(left)).then_with(|| right.cmp(left)));
    tickets
}

pub fn map(queue: &Path, host: &str) -> Mapping {
    let Some(key) = queue.file_name().and_then(|name| name.to_str()) else {
        return Mapping::Unmapped(format!("{} has no queue key", queue.display()));
    };
    let mut refusals = Vec::new();
    for (_, ticket) in recent(queue, host) {
        let found = if ticket.root.is_dir() { checkout(&ticket.root).and_then(|checkout| origin_key(&checkout).map(|found| (checkout, found))) } else { Err(format!("{} no longer exists", ticket.root.display())) };
        match found {
            Ok((checkout, found)) if found == key => return Mapping::Checkout(checkout),
            Ok((checkout, found)) => refusals.push(format!("{} pushes to queue {found}, not {key}", checkout.display())),
            Err(error) => refusals.push(error),
        }
    }
    refusals.dedup();
    Mapping::Unmapped(if refusals.is_empty() { format!("queue {key} holds no ticket from {host}") } else { format!("no ticket's root in queue {key} is a checkout it serves: {}", refusals.join("; ")) })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    pub checkouts: Vec<PathBuf>,
    pub unserved: Vec<String>,
}

pub fn discovered(state_home: &Path, host: &str) -> Discovery {
    let Ok(entries) = std::fs::read_dir(state_home.join("north/land-queue")) else { return Discovery::default() };
    let mut queues: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    queues.sort();
    let mut discovery = Discovery::default();
    for queue in queues {
        match map(&queue, host) {
            Mapping::Checkout(checkout) => discovery.checkouts.push(checkout),
            Mapping::Unmapped(reason) if queue::pending(&queue).is_ok_and(|pending| !pending.is_empty()) => discovery.unserved.push(reason),
            Mapping::Unmapped(_) => {}
        }
    }
    discovery.checkouts.sort();
    discovery.checkouts.dedup();
    discovery
}

pub fn local_host() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}
