use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub name: String,
    pub provider: ProviderName,
    pub account: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub prompt: String,
    pub system_prompt: Option<PathBuf>,
    pub foreground_cap_secs: Option<u64>,
    pub prompt_mode: PromptMode,
    pub idle_exit_minutes: Option<u64>,
    pub parent: Option<String>,
    pub task_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub name: String,
    pub provider: ProviderName,
    pub account: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
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
        }
    }
}
