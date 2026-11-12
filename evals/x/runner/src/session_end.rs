use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::error::{Result, RunError};

const REPORT_WORDS: [&str; 5] = ["Done", "Not done", "Split", "Stop", "Blocked"];
const NUDGE: &str = "[north: your last turn ended with no Done:, Not done:, Split: or Blocked: report and no background command of yours is running] Carry on with the task from where it stopped, and end on your report.";
const SENDER: &str = "service:north-fleet";
pub const USAGE: &str = "eval-x-runner session-end --state DIR NAME";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Report,
    NoReport,
    Stopped,
}

impl End {
    fn label(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::NoReport => "no_report",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    Nudge,
    End(End),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub turns: usize,
    pub in_flight: bool,
    pub reported: bool,
    pub open_tasks: usize,
    pub stopped: bool,
}

impl Seen {
    pub fn step(self, nudged_at: Option<usize>) -> Step {
        if self.reported && !self.in_flight {
            return Step::End(End::Report);
        }
        if self.stopped {
            return Step::End(End::Stopped);
        }
        if self.in_flight || self.turns == 0 || self.open_tasks > 0 {
            return Step::Wait;
        }
        match nudged_at {
            None => Step::Nudge,
            Some(turns) if self.turns > turns => Step::End(End::NoReport),
            Some(_) => Step::Wait,
        }
    }

    // Claude Code stream format: `result` ends a turn and holds its last message, `init` opens one, and `task_started` and `task_notification` bracket a command by `task_id`.
    pub fn read(stream: &str, status: &str) -> Self {
        let mut seen = Self {
            stopped: matches!(status, "exited" | "parked"),
            ..Self::default()
        };
        let mut tasks: Vec<String> = Vec::new();
        for frame in stream
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            let own = frame["parent_tool_use_id"].is_null();
            let task = frame["task_id"].as_str();
            match (frame["type"].as_str(), frame["subtype"].as_str()) {
                (Some("result"), _) => {
                    seen.turns += 1;
                    seen.in_flight = false;
                    seen.reported = is_report(frame["result"].as_str().unwrap_or_default());
                }
                (Some("system"), Some("init")) => seen.in_flight = true,
                (Some("assistant" | "user"), _) if own => seen.in_flight = true,
                (Some("system"), Some("task_started")) => tasks.extend(task.map(str::to_owned)),
                (Some("system"), Some("task_notification")) => {
                    tasks.retain(|open| Some(open.as_str()) != task);
                }
                _ => {}
            }
        }
        seen.open_tasks = tasks.len();
        seen
    }
}

pub fn is_report(reply: &str) -> bool {
    let opening = reply.trim_start_matches(|c: char| c.is_whitespace() || "*#`".contains(c));
    REPORT_WORDS.iter().any(|word| {
        opening
            .strip_prefix(word)
            .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
    })
}

pub fn run(args: &[String]) -> Result<bool> {
    let [flag, state, name] = args else {
        return Err(RunError::Usage(USAGE.to_owned()));
    };
    if flag != "--state" {
        return Err(RunError::Usage(USAGE.to_owned()));
    }
    let state = Path::new(state);
    let seen = Seen::read(
        &fs::read_to_string(stream_file(name)).unwrap_or_default(),
        &status(name),
    );
    let marker = state.join("nudged");
    let nudged_at = fs::read_to_string(&marker)
        .ok()
        .and_then(|turns| turns.trim().parse().ok());
    let mut step = seen.step(nudged_at);
    if step == Step::Nudge {
        if nudged(name) {
            fs::write(&marker, seen.turns.to_string())?;
        } else {
            step = Step::End(End::NoReport);
        }
    }
    let ended = match step {
        Step::End(end) => Some(end.label()),
        Step::Wait | Step::Nudge => None,
    };
    let nudges = usize::from(marker.exists());
    let record = json!({"ended": ended, "turns": seen.turns, "nudges": nudges, "open_tasks": seen.open_tasks});
    let scratch = state.join("end.json.tmp");
    fs::write(&scratch, record.to_string())?;
    fs::rename(scratch, state.join("end.json"))?;
    Ok(ended.is_some())
}

fn stream_file(name: &str) -> PathBuf {
    let state = env::var_os("XDG_STATE_HOME").map_or_else(PathBuf::new, PathBuf::from);
    state.join("north/fleet").join(name).join("g1/stream.jsonl")
}

fn status(name: &str) -> String {
    let shown = Command::new("north")
        .args(["fleet", "show", name, "--json"])
        .output();
    shown
        .ok()
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
        .and_then(|session| session["status"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn nudged(name: &str) -> bool {
    let sent = Command::new("north")
        .args(["fleet", "send", "--from", SENDER, name, NUDGE])
        .status();
    let delivered = sent.as_ref().is_ok_and(|status| status.success());
    if !delivered {
        eprintln!("eval-x-runner: north fleet send to {name} did not deliver: {sent:?}");
    }
    delivered
}
