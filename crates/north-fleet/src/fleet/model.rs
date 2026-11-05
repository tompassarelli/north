use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

pub use super::effort::Effort;
use super::prompt::PromptMode;
use super::sender::Sender;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Proxy,
    Lead,
    Worker,
}

impl Role {
    pub const ALL: [Self; 3] = [Self::Proxy, Self::Lead, Self::Worker];

    pub fn label(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Lead => "lead",
            Self::Worker => "worker",
        }
    }

    pub fn parse(text: &str) -> NorthResult<Self> {
        Self::ALL
            .into_iter()
            .find(|role| role.label() == text)
            .ok_or_else(|| NorthError::Usage(format!("no role {text}; use one of {}", Self::ALL.map(Self::label).join(", "))))
    }
}

impl LaunchRequest {
    pub fn role(&self) -> Role {
        self.role.unwrap_or_else(|| if self.tasks().is_empty() { Role::Lead } else { Role::Worker })
    }

    /// The first of [`Self::tasks`].
    pub fn task(&self) -> Option<String> {
        self.tasks().into_iter().next()
    }

    /// `--task` alone, else every issue the first `Item:` line names, in the prompt or else in the brief it points at.
    pub fn tasks(&self) -> Vec<String> {
        if let Some(task) = &self.task_ref {
            return vec![task.clone()];
        }
        let named = item_issues(&self.prompt);
        if !named.is_empty() {
            return named;
        }
        self.named_brief().map(|brief| item_issues(&brief)).unwrap_or_default()
    }

    /// The minutes of the brief's `ETA:` line, in the prompt or else in the brief it points at.
    pub fn eta_min(&self) -> Option<u64> {
        super::silent::eta_minutes(&self.prompt).or_else(|| self.named_brief().and_then(|brief| super::silent::eta_minutes(&brief)))
    }

    /// The box the brief names, in the prompt or else in the brief it points at: see [`box_name`].
    pub fn box_name(&self) -> Option<String> {
        box_name(&self.prompt).or_else(|| self.named_brief().and_then(|brief| box_name(&brief)))
    }

    /// The `Budget:` line's E, in the prompt or else in the brief it points at.
    pub fn budget(&self) -> Option<super::budget::Stated> {
        super::budget::stated(&self.prompt).or_else(|| self.named_brief().and_then(|brief| super::budget::stated(&brief)))
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

const ITEM_STOPS: [&str; 8] = ["follows", "after", "blocked", "depends", "needs", "see", "refs", "from"];

fn issue_word(word: &str) -> Option<(&str, &str)> {
    let word = word.trim_start_matches(|c: char| "\"'`(<[".contains(c)).trim_end_matches(|c: char| "\"'`)>],;:.".contains(c));
    let (repo, number) = word.rsplit_once('#')?;
    let repo_ok = repo.chars().all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c));
    (repo_ok && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some((repo, number))
}

/// Every issue the first `Item:` line names, in order: `repo#N` or `owner/repo#N`, and a bare `#N` after one for its repository, up to a word such as "follows" that marks the rest as context.
pub fn item_issues(text: &str) -> Vec<String> {
    let Some(line) = text.lines().find_map(|line| line.trim_start().strip_prefix("Item:")) else { return Vec::new() };
    let mut found: Vec<String> = Vec::new();
    let mut repo = "";
    for word in line.split_whitespace() {
        if ITEM_STOPS.contains(&word.trim_matches(|c: char| !c.is_ascii_alphabetic()).to_ascii_lowercase().as_str()) {
            break;
        }
        let Some((named, number)) = issue_word(word) else { continue };
        let named = if named.is_empty() { repo } else { named };
        if named.is_empty() {
            continue;
        }
        repo = named;
        let issue = format!("{named}#{number}");
        if !found.contains(&issue) {
            found.push(issue);
        }
    }
    found
}

/// The brief's `Box:` line, else its first `Item:` line when that names more than its issues; None for a bare `Item: repo#N`.
pub fn box_name(text: &str) -> Option<String> {
    let named = |label: &str| text.lines().find_map(|line| line.trim_start_matches(|c: char| c == '#' || c.is_whitespace()).strip_prefix(label).map(str::trim));
    if let Some(name) = named("Box:").filter(|name| !name.is_empty()) {
        return Some(name.to_owned());
    }
    named("Item:").filter(|item| item.split_whitespace().any(|word| issue_word(word).is_none())).map(str::to_owned)
}

pub fn first_item_issue(text: &str) -> Option<String> {
    item_issues(text).into_iter().next()
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_tasks: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ending>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sender: Option<Sender>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<super::budget::Stated>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ending {
    IdleExit,
    HostEnded,
    Stopped,
}

impl Ending {
    pub fn label(self) -> &'static str {
        match self {
            Self::IdleExit => "idle-exit",
            Self::HostEnded => "host-ended",
            Self::Stopped => "stopped",
        }
    }
}

impl SessionRecord {
    pub fn launched(request: &LaunchRequest, effort: Option<Effort>, conversation: Option<String>, verification: bool, created_ms: u64) -> Self {
        let tasks = request.tasks();
        Self {
            verification,
            name: request.name.clone(),
            provider: request.provider.clone(),
            account: request.account.clone(),
            cwd: request.cwd.clone(),
            model: request.model.clone(),
            effort,
            created_ms,
            host_pid: None,
            conversation,
            host_dir: PathBuf::new(),
            stream_offset: 0,
            stopped: false,
            ended: None,
            system_prompt: request.system_prompt.clone(),
            foreground_cap_secs: request.foreground_cap_secs,
            idle_exit_minutes: request.idle_exit_minutes,
            parent: request.parent.clone(),
            task_ref: tasks.first().cloned(),
            extra_tasks: tasks.iter().skip(1).cloned().collect(),
            prompt_mode: request.prompt_mode,
            system_prompt_sha: None,
            placement: request.placement.clone(),
            machine: request.machine.clone(),
            capacity_need: request.capacity_need.clone(),
            last_sender: None,
            handoff_at: super::handoff::threshold(&request.prompt),
            eta_min: request.eta_min(),
            addressed_since_ms: None,
            role: Some(request.role()),
            prompt_sender: None,
            budget: request.budget(),
            box_name: request.box_name(),
        }
    }

    pub fn role(&self) -> Role {
        self.role.unwrap_or_else(|| match (self.parent.is_some() || self.task_ref.is_some(), self.name == super::runtime::PROXY) {
            (true, _) => Role::Worker,
            (false, true) => Role::Proxy,
            (false, false) => Role::Lead,
        })
    }

    /// Every issue this session holds a claim on: its task and the others its Item line named.
    pub fn tasks(&self) -> impl Iterator<Item = &str> {
        self.task_ref.iter().chain(&self.extra_tasks).map(String::as_str)
    }

    /// The oldest message to this session's name it takes: its own start, or the start of the session it took the name over from.
    pub fn addressed_since(&self) -> u64 {
        self.addressed_since_ms.unwrap_or(self.created_ms)
    }

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
            role: Some(self.role()),
            resume: None,
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

/// A turn that ended on the account's usage limit: the reset the stream carried, in unix seconds, and the limit message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsageLimit {
    pub resets_at_s: Option<u64>,
    pub message: Option<String>,
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
    pub limit: Option<UsageLimit>,
}

impl Default for Transcript {
    fn default() -> Self {
        Self::new()
    }
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
            limit: None,
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
