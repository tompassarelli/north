use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use super::effort::Effort;
pub use super::prompt::PromptMode;
pub use north_fleet_store::status::{Delivery, Report, SessionPhase, Status};
use super::turns::{Turn, Turns};
pub use north_fleet_store::session::{LastSender, Machine, Predicted, ProviderName, Role, SessionRecord, Surface};
pub use north_fleet_store::session_lifecycle::{Launch, Phase, Phases};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub verification: bool,
    /// Keeps the decision gates this session opens out of the owner's queue, digest and gate list; `north gate list --aside` shows them (an eval's sessions open gates on purpose).
    #[serde(default)]
    pub gates_aside: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent: bool,
    /// Declares that the session's shell tool runs its commands in a task container through a relay, never on this host, so the deciders that guard this host's processes let them through; like `--gates-aside`, only an eval's launch sets it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub container_shell: bool,
    /// Where the session's own directory is mounted in that container, from `--container-dir`, which lets a file tool call naming a path there reach the same file through the session's directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_dir: Option<PathBuf>,
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
    /// `north fleet dispatch` started it (`--dispatched`), the only kind of session dispatch stops.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dispatched: bool,
    #[serde(default, skip_serializing_if = "Surface::is_window")]
    pub surface: Surface,
    /// The two-rung route `--route` names (north#1233); a brief's own text never sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

impl LaunchRequest {
    pub fn role(&self) -> Role {
        self.role.unwrap_or_else(|| if self.tasks().is_empty() { Role::Lead } else { Role::Worker })
    }

    pub fn record(&self, effort: Option<Effort>, conversation: Option<String>, verification: bool, created_ms: u64) -> SessionRecord {
        let tasks = self.tasks();
        SessionRecord {
            verification,
            gates_aside: self.gates_aside,
            urgent: self.urgent,
            container_shell: self.container_shell,
            container_dir: self.container_dir.clone(),
            name: self.name.clone(),
            provider: self.provider.clone(),
            account: self.account.clone(),
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            effort,
            overlay: None,
            created_ms,
            host_pid: None,
            conversation,
            host_dir: PathBuf::new(),
            stream_offset: 0,
            system_prompt: self.system_prompt.clone(),
            foreground_cap_secs: self.foreground_cap_secs,
            idle_exit_minutes: self.idle_exit_minutes,
            parent: self.parent.clone(),
            task_ref: tasks.first().cloned(),
            extra_tasks: tasks.iter().skip(1).cloned().collect(),
            prompt_mode: self.prompt_mode,
            system_prompt_sha: None,
            placement: self.placement.clone(),
            machine: self.machine.clone(),
            capacity_need: self.capacity_need.clone(),
            last_sender: None,
            compact_window: self.window(),
            dispatched: self.dispatched,
            surface: self.surface,
            idle_compaction: super::compaction::idle_compaction(self.provider.label()),
            eta_min: self.eta_min(),
            addressed_since_ms: None,
            role: Some(self.role()),
            prompt_sender: None,
            budget: self.budget(),
            box_name: self.box_name(),
            predicted: None,
            claim_box: self.claimed_box(),
            other_claims: self.other_claims(),
            brief: self.brief_path(),
            adopted_ms: None,
            revived_mid_task_ms: None,
            held_unit: None,
            reserve_caller: None,
            route: self.route.clone(),
            first_attempt: super::route::first(self.route.as_deref()).then(|| self.prompt.clone()),
            phase: None,
        }
    }


    pub fn relaunching(record: &SessionRecord) -> Self {
        LaunchRequest {
            verification: record.verification,
            gates_aside: record.gates_aside,
            urgent: record.urgent,
            container_shell: record.container_shell,
            container_dir: record.container_dir.clone(),
            name: record.name.clone(),
            provider: record.provider.clone(),
            account: record.account.clone(),
            cwd: record.cwd.clone(),
            model: record.model.clone(),
            effort: record.effort,
            prompt: String::new(),
            system_prompt: record.system_prompt.clone(),
            foreground_cap_secs: record.foreground_cap_secs,
            prompt_mode: record.prompt_mode,
            idle_exit_minutes: record.idle_exit_minutes,
            parent: record.parent.clone(),
            task_ref: record.task_ref.clone(),
            claim_box: record.claim_box.clone(),
            placement: record.placement.clone(),
            machine: record.machine.clone(),
            capacity_need: record.capacity_need.clone(),
            role: Some(record.role()),
            resume: None,
            compact_window: record.compact_window,
            dispatched: record.dispatched,
            surface: record.surface,
            route: None,
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EntryKind {
    User,
    Assistant,
    Thinking,
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
            EntryKind::Thinking => "thinking",
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
    /// The reply the client is still writing, kept apart from `entries` so every reader of them sees whole messages only.
    pub partial_reply: Option<String>,
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
    pub fn turn_started_ms(&self) -> Option<u64> {
        self.tracker.turn_started_ms()
    }

    pub fn new() -> Self {
        Self {
            name: String::new(),
            status: Status::Starting,
            entries: Vec::new(),
            partial_reply: None,
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

