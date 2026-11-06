use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{follow, send};

const LIVE: [&str; 2] = ["idle", "working"];
const ALARMED: &str = "proxy";
const DROPPED: [&str; 3] = ["Prior report:", "ETA:", "Budget:"];
const SUFFIX: &str = "-back";

pub struct Reverted<'a> {
    pub branch: &'a str,
    pub commit: &'a str,
    pub revert: &'a str,
    pub red: &'a str,
    pub failing: &'a str,
    pub log: &'a Path,
}

pub struct Row {
    pub status: String,
    pub task: Option<String>,
    pub parent: Option<String>,
    pub brief: String,
}

pub trait Fleet {
    fn row(&self, session: &str) -> Option<Row>;
    fn send(&self, session: &str, text: &str) -> bool;
    fn launch(&self, name: &str, row: &Row, prompt: &str) -> Result<(), String>;
}

impl Reverted<'_> {
    fn short(&self) -> &str {
        &self.commit[..self.commit.len().min(9)]
    }

    fn evidence(&self) -> String {
        format!("main went red at {} on {}; main without {} ({}) passes them, so the sheriff queued its revert {} ahead of the queue; log: {}", &self.red[..self.red.len().min(9)], self.failing, self.branch, self.short(), self.revert, self.log.display())
    }

    fn notice(&self) -> String {
        format!("north land: {}; fix the lane and queue it again with Follows: {}", self.evidence(), self.short())
    }

    fn brief(&self, brief: &str) -> String {
        let kept: Vec<&str> = brief.lines().filter(|line| !DROPPED.iter().any(|dropped| line.starts_with(dropped))).collect();
        let header = kept.iter().position(|line| line.trim().is_empty()).unwrap_or(kept.len());
        let mut lines: Vec<String> = kept[..header].iter().map(|line| (*line).to_owned()).collect();
        lines.push(format!("Follows: {} landed as {} and was reverted by {}", self.branch, self.commit, self.revert));
        lines.push(format!("Prior report: the sheriff reverted this box's landed work: {}. Start a lane from origin/main, bring the work back with `git cherry-pick {}`, fix what fails, and queue it with Follows: {}.", self.evidence(), self.commit, self.short()));
        lines.extend(kept[header..].iter().map(|line| (*line).to_owned()));
        lines.join("\n")
    }
}

fn restaffed(session: &str, commit: &str) -> String {
    let base = session.rsplit_once(SUFFIX).filter(|(_, tail)| tail.strip_prefix('-').is_some_and(|sha| sha.len() == 7 && sha.chars().all(|c| c.is_ascii_hexdigit()))).map_or(session, |(base, _)| base);
    format!("{base}{SUFFIX}-{}", &commit[..commit.len().min(7)])
}

pub fn tell(fleet: &dyn Fleet, session: Option<&str>, reverted: &Reverted) -> String {
    let Some(session) = session.filter(|session| !session.is_empty()) else {
        let text = format!("{}; no fleet session owns {}, so nobody is told to bring it back", reverted.notice(), reverted.branch);
        return if fleet.send(ALARMED, &text) { format!("told {ALARMED}: no session owns {}", reverted.branch) } else { format!("could not tell {ALARMED} that no session owns {}", reverted.branch) };
    };
    let row = fleet.row(session);
    if let Some(row) = &row
        && LIVE.contains(&row.status.as_str())
        && fleet.send(session, &reverted.notice())
    {
        return format!("told {session} that {} was reverted", reverted.branch);
    }
    let Some(row) = row.filter(|row| !row.brief.is_empty()) else {
        let text = format!("{}; its session {session} is gone and left no brief, so restaff its box by hand", reverted.notice());
        return if fleet.send(ALARMED, &text) { format!("told {ALARMED}: {session} left no brief") } else { format!("could not tell {ALARMED} that {session} left no brief") };
    };
    let name = restaffed(session, reverted.commit);
    match fleet.launch(&name, &row, &reverted.brief(&row.brief)) {
        Ok(()) => {
            fleet.send(ALARMED, &format!("north land: {session} is {}, so the sheriff restaffed its box as {name} to bring back {}; watch its first turn", row.status, reverted.branch));
            format!("restaffed {session} as {name}")
        }
        Err(error) => {
            fleet.send(ALARMED, &format!("{}; its session {session} is {} and restaffing it as {name} failed ({error}), so restaff its box by hand", reverted.notice(), row.status));
            format!("could not restaff {session}: {error}")
        }
    }
}

pub struct Cli;

impl Fleet for Cli {
    fn row(&self, session: &str) -> Option<Row> {
        let shown = Command::new(follow::program()).args(["fleet", "show", session, "--json"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
        let row: Value = serde_json::from_slice(&shown.stdout).ok()?;
        let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        let brief = row.get("entries").and_then(Value::as_array).and_then(|entries| entries.iter().find(|entry| entry.get("kind").and_then(Value::as_str) == Some("user"))).and_then(|entry| entry.get("text")).and_then(Value::as_str).unwrap_or_default().to_owned();
        Some(Row { status: text("status").unwrap_or_default(), task: text("task_ref"), parent: text("parent"), brief })
    }

    fn send(&self, session: &str, text: &str) -> bool {
        send(session, text)
    }

    fn launch(&self, name: &str, row: &Row, prompt: &str) -> Result<(), String> {
        let mut command = Command::new(follow::program());
        command.args(["fleet", "launch", "--provider", "claude", "--role", "worker", "--name", name, "--no-wait", "--parent", row.parent.as_deref().unwrap_or(ALARMED)]);
        if let Some(task) = &row.task {
            command.args(["--task", task]);
        }
        let launched = command.arg(prompt).stdin(Stdio::null()).env_remove("NORTH_RUNTIME_LINK").output().map_err(|error| error.to_string())?;
        if launched.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&launched.stderr).trim().to_owned()) }
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
