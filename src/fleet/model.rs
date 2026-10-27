use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use super::effort::Effort;
use super::prompt::PromptMode;
use super::turns::{Turn, Turns};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderName(String);

impl ProviderName {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn label(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub provider: ProviderName,
    pub label: String,
    pub home: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_file: Option<PathBuf>,
}

/// Another machine a session's provider CLI runs on, reached through `ssh`; the session host and
/// transcript stay on this one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    pub ssh: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub verification: bool,
    pub name: String,
    pub provider: ProviderName,
    pub account: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<Effort>,
    pub prompt: String,
    pub system_prompt: Option<PathBuf>,
    pub foreground_cap_secs: Option<u64>,
    pub prompt_mode: PromptMode,
    pub idle_exit_minutes: Option<u64>,
    pub parent: Option<String>,
    pub task_ref: Option<String>,
    /// Why the launch overrode placement (`local: REASON`, `pin: REASON`), shown in `fleet list`.
    pub placement: Option<String>,
    pub machine: Option<Machine>,
    #[serde(default)]
    pub capacity_need: Option<String>,
}

impl LaunchRequest {
    /// `--task`, else the issue the first `Item:` line starts with (`repo#N` or `owner/repo#N`),
    /// in the prompt or else in the `.md` brief at an absolute path the prompt names, so a session
    /// launched without `--task` still holds its issue.
    pub fn task(&self) -> Option<String> {
        self.task_ref.clone().or_else(|| first_item_issue(&self.prompt)).or_else(|| first_item_issue(&self.named_brief()?))
    }

    pub fn named_brief(&self) -> Option<String> {
        let brief = self
            .prompt
            .split_whitespace()
            .map(|word| word.trim_start_matches(|c: char| "\"'`(<[".contains(c)).trim_end_matches(|c: char| "\"'`)>],;:.".contains(c)))
            .map(Path::new)
            .find(|path| path.is_absolute() && path.extension().is_some_and(|ext| ext == "md") && path.is_file())?;
        std::fs::read_to_string(brief).ok()
    }
}

pub(crate) fn first_item_issue(text: &str) -> Option<String> {
    let line = text.lines().find_map(|line| line.trim_start().strip_prefix("Item:"))?;
    let word = line.split_whitespace().next()?.trim_end_matches([':', ',', '.', ';']);
    let (repo, number) = word.rsplit_once('#')?;
    let repo_ok = !repo.is_empty() && repo.chars().all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c));
    (repo_ok && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then(|| word.to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(default)]
    pub verification: bool,
    pub name: String,
    pub provider: ProviderName,
    pub account: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    pub created_ms: u64,
    pub host_pid: Option<u32>,
    pub conversation: Option<String>,
    #[serde(default)]
    pub host_dir: PathBuf,
    #[serde(default)]
    pub stream_offset: u64,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub system_prompt: Option<PathBuf>,
    #[serde(default)]
    pub foreground_cap_secs: Option<u64>,
    #[serde(default)]
    pub idle_exit_minutes: Option<u64>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub task_ref: Option<String>,
    #[serde(default)]
    pub prompt_mode: PromptMode,
    #[serde(default)]
    pub system_prompt_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<Machine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_need: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sender: Option<LastSender>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_at: Option<u64>,
}

impl SessionRecord {
    pub fn relaunch_request(&self) -> LaunchRequest {
        LaunchRequest {
            verification: self.verification,
            name: self.name.clone(),
            provider: self.provider.clone(),
            account: self.account.clone(),
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            effort: self.effort,
            prompt: String::new(),
            system_prompt: self.system_prompt.clone(),
            foreground_cap_secs: self.foreground_cap_secs,
            prompt_mode: self.prompt_mode,
            idle_exit_minutes: self.idle_exit_minutes,
            parent: self.parent.clone(),
            task_ref: self.task_ref.clone(),
            placement: self.placement.clone(),
            machine: self.machine.clone(),
            capacity_need: self.capacity_need.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LastSender {
    pub name: String,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Queued,
    Now,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Starting,
    Working,
    Idle,
    Parked,
    Exited,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Starting => "starting",
            Status::Working => "working",
            Status::Idle => "idle",
            Status::Parked => "parked",
            Status::Exited => "exited",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    User,
    Assistant,
    Tool,
    ToolResult,
    TurnEnd,
    Error,
    Notice,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::User => "user",
            EntryKind::Assistant => "assistant",
            EntryKind::Tool => "tool",
            EntryKind::ToolResult => "result",
            EntryKind::TurnEnd => "turn",
            EntryKind::Error => "error",
            EntryKind::Notice => "notice",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: u64,
}

impl QuotaWindow {
    pub fn spanning(minutes: u64, used_percent: u64) -> Self {
        let label = match minutes {
            0 => "?".to_owned(),
            _ if minutes.is_multiple_of(1440) => format!("{}d", minutes / 1440),
            _ if minutes.is_multiple_of(60) => format!("{}h", minutes / 60),
            _ => format!("{minutes}m"),
        };
        Self { label, used_percent }
    }

    pub fn left_percent(&self) -> u64 {
        100 - self.used_percent.min(100)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Quota {
    pub windows: Vec<QuotaWindow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub name: String,
    pub status: Status,
    pub entries: Vec<Entry>,
    pub conversation: Option<String>,
    pub last_activity_ms: Option<u64>,
    pub active_turn: Option<String>,
    pub turns: Vec<Turn>,
    pub tracker: Turns,
    pub quota: Option<Quota>,
}

impl Transcript {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            status: Status::Starting,
            entries: Vec::new(),
            conversation: None,
            last_activity_ms: None,
            active_turn: None,
            turns: Vec::new(),
            tracker: Turns::default(),
            quota: None,
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
