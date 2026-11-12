use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Result, RunError};

pub const USAGE: &str = "eval-x-runner plain-end --state DIR";
const QUIET_POLLS: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub runs: usize,
    pub running: bool,
    pub asking: bool,
    pub open_tasks: usize,
    pub events: usize,
}

impl Seen {
    // T3 Code thread read: `thread.activeRunId` is set while a run executes, `thread.runCount` counts runs, and a wake run that Claude Code starts for a finished background command is a run of its own.
    // Claude Code session file: a tool result carrying `toolUseResult.backgroundTaskId` opens a command, and the `queue-operation` entry holding `<task-notification>` with the same `<task-id>` closes it.
    pub fn read(shown: &str, transcript: &str) -> Self {
        let shown: Value = serde_json::from_str(shown).unwrap_or(Value::Null);
        let thread = &shown["thread"];
        let mut open: Vec<String> = Vec::new();
        let mut events = 0;
        for line in transcript
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            events += 1;
            if let Some(task) = line["toolUseResult"]["backgroundTaskId"].as_str() {
                open.push(task.to_owned());
            }
            if line["type"] == "queue-operation"
                && let Some(content) = line["content"].as_str()
                && content.starts_with("<task-notification>")
                && let Some(task) = tag(content, "task-id")
            {
                open.retain(|candidate| candidate != task);
            }
        }
        Self {
            runs: thread["runCount"]
                .as_u64()
                .map_or(0, |runs| usize::try_from(runs).unwrap_or(usize::MAX)),
            running: !thread["activeRunId"].is_null(),
            asking: thread["pendingRequestCount"].as_u64().unwrap_or(0) > 0,
            open_tasks: open.len(),
            events,
        }
    }

    fn idle(self) -> bool {
        !self.running && self.open_tasks == 0
    }
}

fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let (_, rest) = text.split_once(&format!("<{name}>"))?;
    rest.split_once(&format!("</{name}>")).map(|(value, _)| value)
}

pub fn run(args: &[String]) -> Result<bool> {
    let [flag, state] = args else {
        return Err(RunError::Usage(USAGE.to_owned()));
    };
    if flag != "--state" {
        return Err(RunError::Usage(USAGE.to_owned()));
    }
    let state = Path::new(state);
    let seen = Seen::read(
        &fs::read_to_string(state.join("show.json")).unwrap_or_default(),
        &fs::read_to_string(state.join("transcript.jsonl")).unwrap_or_default(),
    );
    let marker = state.join("quiet");
    let before: Vec<usize> = fs::read_to_string(&marker)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    let quiet = match before[..] {
        [runs, events, quiet] if seen.idle() && (runs, events) == (seen.runs, seen.events) => {
            quiet + 1
        }
        _ => 0,
    };
    fs::write(&marker, format!("{} {} {quiet}", seen.runs, seen.events))?;
    let ended = if seen.running {
        None
    } else if seen.asking {
        Some("asking")
    } else if quiet >= QUIET_POLLS {
        Some("idle")
    } else {
        None
    };
    let record = json!({"ended": ended, "turns": seen.runs, "nudges": 0, "open_tasks": seen.open_tasks});
    let scratch = state.join("end.json.tmp");
    fs::write(&scratch, record.to_string())?;
    fs::rename(scratch, state.join("end.json"))?;
    Ok(ended.is_some())
}
