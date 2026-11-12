use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

pub use super::effort::Effort;
use super::prompt::PromptMode;
pub use north_fleet_store::status::{Delivery, Status};
use super::sender::Sender;
use super::turns::{Turn, Turns};
pub use north_session_host::session_lifecycle::{Launch, Phase, Phases};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiKey>,
}

/// The organization key an API-billed account runs on: the pool entry's name and the file holding its key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiKey {
    pub org: String,
    pub file: PathBuf,
}

impl Account {
    /// The label a session records: an API-billed account's label with the organization it ran on, so a row names which key was spent.
    pub fn recorded_label(&self) -> String {
        match &self.api {
            Some(key) if !self.label.contains(':') => format!("{}:{}", self.label, key.org),
            _ => self.label.clone(),
        }
    }
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
    /// The box of the task this launch works alone, from `--box`, which its claim covers instead of the whole issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_box: Option<String>,
    /// Why the launch overrode placement (`local: REASON`, `pin: REASON`), shown in `fleet list`.
    pub placement: Option<String>,
    pub machine: Option<Machine>,
    #[serde(default)]
    pub capacity_need: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    /// The auto-compact window a relaunch keeps from the session it restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_window: Option<u64>,
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

    /// `--task` alone, else every issue the first `Item:` line names, else the issue the brief's body opens with, in the prompt or else in the brief it points at.
    pub fn tasks(&self) -> Vec<String> {
        if let Some(task) = &self.task_ref {
            return vec![task.clone()];
        }
        let brief = self.named_brief();
        let texts = || std::iter::once(self.prompt.as_str()).chain(brief.as_deref());
        texts().map(item_issues).find(|named| !named.is_empty()).or_else(|| texts().find_map(lead_issue).map(|issue| vec![issue])).unwrap_or_default()
    }

    /// The first `Item:` line, in the prompt or else in the brief it points at: what a box-scoped gate's text is matched against, beside [`Self::claimed_box`].
    pub fn item_line(&self) -> Option<String> {
        first_item_line(&self.prompt).or_else(|| self.named_brief().and_then(|brief| first_item_line(&brief)))
    }

    /// The minutes of the brief's `ETA:` line, in the prompt or else in the brief it points at.
    pub fn eta_min(&self) -> Option<u64> {
        super::silent::eta_minutes(&self.prompt).or_else(|| self.named_brief().and_then(|brief| super::silent::eta_minutes(&brief)))
    }

    /// The box this launch claims: `--box`, else the brief's own `Box:` line, never the Item line [`box_name`] falls back to.
    pub fn claimed_box(&self) -> Option<String> {
        self.claim_box.clone().or_else(|| boxed(&self.prompt)).or_else(|| self.named_brief().and_then(|brief| boxed(&brief)))
    }

    /// The claim keys of the other boxes the prompt and the brief it points at name: see [`other_box_claims`].
    pub fn other_claims(&self) -> Vec<String> {
        let brief = self.named_brief();
        let mut keys: Vec<String> = Vec::new();
        for key in std::iter::once(self.prompt.as_str()).chain(brief.as_deref()).flat_map(other_box_claims) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    /// The box the brief names, in the prompt or else in the brief it points at: see [`box_name`].
    pub fn box_name(&self) -> Option<String> {
        box_name(&self.prompt).or_else(|| self.named_brief().and_then(|brief| box_name(&brief)))
    }

    /// The auto-compact window the session runs with: the one a relaunch keeps, else the `Arm: context-NNNk` stamp in the prompt or the brief it points at.
    pub fn window(&self) -> Option<u64> {
        self.compact_window.or_else(|| super::compaction::window(&self.prompt)).or_else(|| self.named_brief().and_then(|brief| super::compaction::window(&brief)))
    }

    /// The `Budget:` line's E, in the prompt or else in the brief it points at.
    pub fn budget(&self) -> Option<super::budget::Stated> {
        super::budget::stated(&self.prompt).or_else(|| self.named_brief().and_then(|brief| super::budget::stated(&brief)))
    }

    /// The brief file the prompt points at: its first absolute path to an existing `.md` file.
    pub fn brief_path(&self) -> Option<PathBuf> {
        self
            .prompt
            .split_whitespace()
            .map(|word| word.trim_start_matches(|c: char| "\"'`(<[".contains(c)).trim_end_matches(|c: char| "\"'`)>],;:.".contains(c)))
            .map(Path::new)
            .find(|path| path.is_absolute() && path.extension().is_some_and(|ext| ext == "md") && path.is_file())
            .map(Path::to_path_buf)
    }

    pub fn named_brief(&self) -> Option<String> {
        std::fs::read_to_string(self.brief_path()?).ok()
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
fn first_item_line(text: &str) -> Option<String> {
    text.lines().find_map(|line| line.trim_start().strip_prefix("Item:")).map(str::to_owned)
}

pub fn item_issues(text: &str) -> Vec<String> {
    let Some(line) = first_item_line(text) else { return Vec::new() };
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

/// The issue the brief's first line after its `Key: value` header opens with, as `north#855: run the frontier scoreboard` does: the work a launch with no `--task` and no `Item:` line holds.
fn lead_issue(text: &str) -> Option<String> {
    let facet = |line: &str| line.split_once(':').is_some_and(|(key, _)| key.starts_with(|c: char| c.is_ascii_uppercase()) && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let line = text.lines().map(str::trim).find(|line| !line.is_empty() && !facet(line))?;
    let (repo, number) = issue_word(line.split_whitespace().next()?)?;
    (!repo.is_empty()).then(|| format!("{repo}#{number}"))
}

/// The text after `label` on the first line of `text` that starts with it, ignoring leading `#` and spaces.
pub fn named_line<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.trim_start_matches(|c: char| c == '#' || c.is_whitespace()).strip_prefix(label).map(str::trim))
}

/// The brief's own non-empty `Box:` line.
pub fn boxed(text: &str) -> Option<String> {
    named_line(text, "Box:").filter(|name| !name.is_empty()).map(str::to_owned)
}

/// The brief's `Box:` line, else its first `Item:` line when that names more than its issues; None for a bare `Item: repo#N`.
pub fn box_name(text: &str) -> Option<String> {
    boxed(text).or_else(|| named_line(text, "Item:").filter(|item| item.split_whitespace().any(|word| issue_word(word).is_none())).map(str::to_owned))
}

/// The claim keys of the boxes `text` names past its first `Box:` and first `Item:` line, which name the launch's own: each further such line that opens with `repo#N` and goes on with the box's text claims that box of that issue.
pub fn other_box_claims(text: &str) -> Vec<String> {
    let (mut boxes, mut items) = (0, 0);
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches(|c: char| c == '#' || c.is_whitespace());
            let (seen, value) = match (line.strip_prefix("Box:"), line.strip_prefix("Item:")) {
                (Some(value), _) => (&mut boxes, value),
                (_, Some(value)) => (&mut items, value),
                _ => return None,
            };
            *seen += 1;
            let (issue, box_text) = value.trim().split_once(char::is_whitespace)?;
            let (repo, number) = issue_word(issue).filter(|(repo, _)| !repo.is_empty())?;
            (*seen > 1 && !box_text.trim().is_empty()).then(|| super::claims::claim_key(&format!("{repo}#{number}"), Some(box_text.trim())))
        })
        .collect()
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<String>,
    pub created_ms: u64,
    pub host_pid: Option<u32>,
    pub conversation: Option<String>,
    #[serde(default)]
    pub host_dir: PathBuf,
    #[serde(default)]
    pub stream_offset: u64,
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
    pub compact_window: Option<u64>,
    /// Whether Claude Code compacts the session on idle (on by default since v2.1.299), recorded beside its window so the window arm's cost can be read with and without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_compaction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_min: Option<u64>,
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
    /// The box of its task its claim covers instead of the whole issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_box: Option<String>,
    /// The claim keys of the other boxes its brief names, held beside its own task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_claims: Vec<String>,
    /// The brief file its launch prompt pointed at, which a handover hands the new conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revived_mid_task_ms: Option<u64>,
    /// The held unit on its machine when the session took a spare, whose name is not the one its session name gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_unit: Option<String>,
    /// The brief of a two-rung route's first attempt, which a turn that stops short hands the second rung with what it tried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_attempt: Option<String>,
    /// The phase this host's lifecycle store gave the launch when the record was read; session.json never holds it.
    #[serde(skip)]
    pub phase: Option<Phase>,
}

impl SessionRecord {
    pub fn adopted(&self) -> bool {
        self.adopted_ms.is_some()
    }

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
            overlay: None,
            created_ms,
            host_pid: None,
            conversation,
            host_dir: PathBuf::new(),
            stream_offset: 0,
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
            compact_window: request.window(),
            idle_compaction: super::compaction::idle_compaction(request.provider.label()),
            eta_min: request.eta_min(),
            addressed_since_ms: None,
            role: Some(request.role()),
            prompt_sender: None,
            budget: request.budget(),
            box_name: request.box_name(),
            claim_box: request.claimed_box(),
            other_claims: request.other_claims(),
            brief: request.brief_path(),
            adopted_ms: None,
            revived_mid_task_ms: None,
            held_unit: None,
            first_attempt: super::rungs::first(&request.prompt).then(|| request.prompt.clone()),
            phase: None,
        }
    }

    pub fn launch(&self) -> Launch<'_> {
        Launch { name: &self.name, created_ms: self.created_ms, adopted: self.adopted(), host_pid: self.host_pid }
    }

    /// The phase this host's lifecycle store gave the launch when the record was read; `None` for a record built in
    /// memory and never read back.
    pub fn phase(&self) -> Option<Phase> {
        self.phase
    }

    /// The session ended: stopped, ended by its host or retired.
    pub fn ended(&self) -> bool {
        self.phase.is_some_and(Phase::ended)
    }

    /// A live host serves the session.
    pub fn live(&self) -> bool {
        self.phase == Some(Phase::Live)
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

    /// Adds `task` to the issues this session holds, as a launch's `--task` would have; false when it held it already.
    pub fn hold(&mut self, task: &str) -> bool {
        if self.tasks().any(|held| held == task) {
            return false;
        }
        if self.task_ref.is_none() {
            self.task_ref = Some(task.to_owned());
        } else {
            self.extra_tasks.push(task.to_owned());
        }
        true
    }

    /// Drops `task` from the issues this session holds, with the box its launch named on it and the boxes its brief named of it; false when it held none of them.
    pub fn release(&mut self, task: &str) -> bool {
        let before = (self.task_ref.clone(), self.extra_tasks.len(), self.other_claims.len());
        if self.task_ref.as_deref() == Some(task) {
            self.claim_box = None;
            self.task_ref = (!self.extra_tasks.is_empty()).then(|| self.extra_tasks.remove(0));
        }
        self.extra_tasks.retain(|held| held != task);
        self.other_claims.retain(|key| super::claims::split_key(key).0 != task);
        before != (self.task_ref.clone(), self.extra_tasks.len(), self.other_claims.len())
    }

    /// The keys of the claims this session holds: each task, the first as the box its launch named.
    pub fn claim_keys(&self) -> Vec<String> {
        let tasks: Vec<String> = self.tasks().map(str::to_owned).collect();
        super::claims::claim_keys(&tasks, self.claim_box.as_deref(), &self.other_claims)
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
            claim_box: self.claim_box.clone(),
            placement: self.placement.clone(),
            machine: self.machine.clone(),
            capacity_need: self.capacity_need.clone(),
            role: Some(self.role()),
            resume: None,
            compact_window: self.compact_window,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LastSender {
    pub name: String,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EntryKind {
    User,
    Assistant,
    Tool,
    ToolResult,
    TurnEnd,
    Error,
    /// A provider's safety classifier flagged a turn and refused it or moved it to a fallback model.
    Refusal,
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
            EntryKind::Refusal => "refusal",
            EntryKind::Notice => "notice",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

