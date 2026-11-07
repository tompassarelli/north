use std::fs;
use std::path::{Path, PathBuf};

use crate::platform::{land_queue, records_mirror};
use serde_json::{Value, json};

use super::git;

const USAGE: &str = "usage: north land clear-withdrawal TICKET --reason TEXT [--repo DIR] [--noticed-by WHO]";
const WITHDRAWN: &str = "withdrawn";
const OUTBOX: &str = "outbox";
const BY_PREFIX: &str = "withdrawn by ";
const LANDER: &str = "north-land";

struct Withdrawn {
    path: PathBuf,
    ticket: Value,
}

impl Withdrawn {
    fn text(&self, key: &str) -> Option<&str> {
        self.ticket[key].as_str()
    }

    fn queued_micros(&self) -> u128 {
        let named = self.path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok());
        self.ticket["queued_micros"].as_u64().map(u128::from).or(named).unwrap_or(0)
    }

    fn name(&self) -> String {
        self.path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
    }
}

fn withdrawn(queue: &Path) -> Result<Vec<Withdrawn>, String> {
    let mut found = Vec::new();
    for directory in [queue.join(WITHDRAWN), queue.join(OUTBOX).join(WITHDRAWN)] {
        let Ok(entries) = fs::read_dir(&directory) else { continue };
        for path in entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")) {
            let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let ticket = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
            found.push(Withdrawn { path, ticket });
        }
    }
    Ok(found)
}

/// Why the session `session` may not queue `head` of `branch` in `root` again: the lane's newest withdrawn ticket is at
/// that head, was withdrawn on someone's call (the lander's own withdrawals tell the session to queue again) and no one
/// has cleared it. A changed head is a new ticket.
pub fn refuse_requeue(queue: &Path, root: &Path, branch: &str, head: &str, session: Option<&str>) -> Result<(), String> {
    let lander = format!("{BY_PREFIX}{LANDER}:");
    let newest = withdrawn(queue)?.into_iter().filter(|found| found.text("root").is_some_and(|path| Path::new(path) == root) && found.text("branch") == Some(branch)).max_by_key(Withdrawn::queued_micros);
    let Some(found) = newest else { return Ok(()) };
    let verdict = found.text("verdict").filter(|verdict| verdict.starts_with(BY_PREFIX) && !verdict.starts_with(&lander));
    match verdict {
        Some(verdict) if found.text("head") == Some(head) && found.text("session") == session && found.text("cleared").is_none() => {
            Err(format!("{branch} ({head}) was {verdict}; whoever withdrew it clears it with `north land clear-withdrawal {} --reason TEXT`, or the lane's head changes", found.name()))
        }
        _ => Ok(()),
    }
}

pub fn clear(queue: &Path, wanted: &str, noticed_by: &str, reason: &str) -> Result<String, String> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    let named = withdrawn(queue)?.into_iter().filter(|found| found.path.file_name().is_some_and(|name| name == file.as_str()) || found.text("branch") == Some(wanted)).max_by_key(Withdrawn::queued_micros);
    let mut found = named.ok_or_else(|| format!("no withdrawn ticket named {wanted} in {}", queue.display()))?;
    let cleared = format!("cleared by {noticed_by}: {}", reason.trim());
    found.ticket["cleared"] = json!(cleared);
    let text = serde_json::to_string_pretty(&found.ticket).map_err(|error| error.to_string())? + "\n";
    fs::write(&found.path, text).map_err(|error| format!("{}: {error}", found.path.display()))?;
    Ok(format!("north land: {} ({}) {cleared}; its session may queue it again", found.text("branch").unwrap_or_default(), found.text("head").unwrap_or_default()))
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut ticket, mut reason, mut noticed_by, mut repo, mut valid) = (String::new(), String::new(), "proxy".to_owned(), PathBuf::from("."), true);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let value = |rest: &mut std::slice::Iter<String>| rest.next().cloned().unwrap_or_default();
        match argument.as_str() {
            "--reason" => reason = value(&mut rest),
            "--noticed-by" => noticed_by = value(&mut rest),
            "--repo" => repo = PathBuf::from(value(&mut rest)),
            word if word.starts_with('-') || !ticket.is_empty() => valid = false,
            word => word.clone_into(&mut ticket),
        }
    }
    if !valid || ticket.is_empty() || reason.trim().is_empty() || noticed_by.is_empty() {
        eprintln!("{USAGE}");
        return 64;
    }
    let cleared = git::text(&repo, &["remote", "get-url", "--push", "origin"])
        .ok_or_else(|| "origin has no push URL".to_owned())
        .and_then(|origin| clear(&land_queue::directory(&records_mirror::state_home(), origin.trim()), &ticket, &noticed_by, &reason));
    match cleared {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north land clear-withdrawal: {error}");
            1
        }
    }
}
