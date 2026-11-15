use std::path::Path;
use std::process::Command;

use crate::box_tick::SHORT_SHA;
pub use crate::box_tick::{Tick, closable, needs_observation, tick};
use crate::issue_body::{self, Edit, Issue};

#[derive(Debug, PartialEq, Eq)]
pub struct Trailer {
    pub item: String,
    pub box_text: String,
}

pub fn trailers(message: &str) -> Result<Vec<Trailer>, String> {
    let mut item: Option<&str> = None;
    let mut found = Vec::new();
    for line in message.lines() {
        if let Some(value) = line.strip_prefix("Item:") {
            item = value.split_whitespace().next();
        } else if let Some(value) = line.strip_prefix("Box:") {
            let box_text = value.trim();
            let Some(item) = item else {
                return Err(format!("Box: {box_text} has no Item: line before it"));
            };
            Issue::parse(item).map_err(|error| format!("Item: {error}"))?;
            if box_text.is_empty() {
                return Err(format!("Item: {item} has an empty Box: line"));
            }
            found.push(Trailer { item: item.to_owned(), box_text: box_text.to_owned() });
        }
    }
    Ok(found)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Landed {
    pub issue: Issue,
    pub box_text: String,
    pub sha: String,
}

pub fn commits(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, String)>, String> {
    let range = format!("{base}..{head}");
    let output = Command::new("git").arg("-C").arg(repo).args(["log", "--reverse", "--format=%H%x1f%B%x1e", &range]).output().map_err(|error| format!("git log {range}: {error}"))?;
    if !output.status.success() {
        return Err(format!("git log {range}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\x1e')
        .filter_map(|record| record.trim_start_matches('\n').split_once('\x1f'))
        .map(|(sha, message)| (sha.to_owned(), message.to_owned()))
        .collect())
}

pub fn landed(repo: &Path, base: &str, head: &str) -> Result<Vec<Landed>, String> {
    let mut found = Vec::new();
    for (sha, message) in commits(repo, base, head)? {
        for trailer in trailers(&message).map_err(|error| format!("{}: {error}", &sha[..sha.len().min(SHORT_SHA)]))? {
            let issue = Issue::parse(&trailer.item)?;
            found.push(Landed { issue, box_text: trailer.box_text, sha: sha.clone() });
        }
    }
    Ok(found)
}

/// A box a sheriff revert names in the landing it reverts.
#[derive(Debug, PartialEq, Eq)]
pub struct Reverted {
    pub issue: Issue,
    pub box_text: String,
    pub sha: String,
}

pub fn reverts(repo: &Path, base: &str, head: &str) -> Result<Vec<Reverted>, String> {
    let mut found = Vec::new();
    for (_, message) in commits(repo, base, head)? {
        for sha in message.lines().filter_map(|line| line.strip_prefix("Sheriff-Reverts:")).map(str::trim).filter(|sha| !sha.is_empty()) {
            let output = Command::new("git").arg("-C").arg(repo).args(["log", "-1", "--format=%B", sha]).output().map_err(|error| format!("git log {sha}: {error}"))?;
            if !output.status.success() {
                return Err(format!("git log {sha}: {}", String::from_utf8_lossy(&output.stderr).trim()));
            }
            for trailer in trailers(&String::from_utf8_lossy(&output.stdout))? {
                found.push(Reverted { issue: Issue::parse(&trailer.item)?, box_text: trailer.box_text, sha: sha.to_owned() });
            }
        }
    }
    Ok(found)
}

pub fn revert_queue(queue_directory: &Path, repo: &Path, base: &str, head: &str) -> Vec<String> {
    let reverted = match reverts(repo, base, head) {
        Ok(reverted) => reverted,
        Err(error) => return vec![format!("no box unticked: {error}")],
    };
    reverted
        .into_iter()
        .map(|reverted| {
            let edit = Edit::Untick { box_text: reverted.box_text.clone(), sha: reverted.sha.clone() };
            match issue_body::submit(queue_directory, &reverted.issue, &edit) {
                Ok(_) => format!("queued the untick of {} box {:?} reverted from {}", reverted.issue, reverted.box_text, &reverted.sha[..reverted.sha.len().min(SHORT_SHA)]),
                Err(error) => format!("cannot queue the untick of {} box {:?}: {error}", reverted.issue, reverted.box_text),
            }
        })
        .collect()
}

pub fn queue(queue_directory: &Path, repo: &Path, base: &str, head: &str) -> Vec<String> {
    let landed = match landed(repo, base, head) {
        Ok(landed) => landed,
        Err(error) => return vec![format!("no box ticked: {error}")],
    };
    landed
        .into_iter()
        .map(|landed| {
            let edit = Edit::Land { box_text: landed.box_text.clone(), sha: landed.sha.clone() };
            match issue_body::submit(queue_directory, &landed.issue, &edit) {
                Ok(_) => format!("queued the tick of {} box {:?} landed in {}", landed.issue, landed.box_text, &landed.sha[..landed.sha.len().min(SHORT_SHA)]),
                Err(error) => format!("cannot queue the tick of {} box {:?}: {error}", landed.issue, landed.box_text),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tests;
