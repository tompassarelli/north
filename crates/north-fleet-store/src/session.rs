use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::claim_key::{claim_keys, split_key};
use crate::effort::Effort;
use crate::error::{NorthError, NorthResult};
use crate::prompt_mode::PromptMode;
use crate::sender::Sender;
use crate::stated_budget::Stated;
pub use crate::session_lifecycle::{Launch, Phase, Phases};

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

/// Another machine a session's provider CLI runs on, reached through `ssh`; the session host and
/// transcript stay on this one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    pub ssh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ssh: Option<String>,
}

impl Machine {
    pub fn session_login(&self) -> &str {
        self.agent_ssh.as_deref().unwrap_or(&self.ssh)
    }

    pub fn session_home(&self, operator_home: &Path) -> Option<PathBuf> {
        self.agent_ssh.as_deref().and_then(|login| Self::login_home(login, operator_home))
    }

    pub fn session_path(&self, path: &Path, operator_home: &Path) -> PathBuf {
        match (self.session_home(operator_home), path.strip_prefix(operator_home)) {
            (Some(home), Ok(rest)) => home.join(rest),
            _ => path.to_path_buf(),
        }
    }

    /// The home of the Unix user a session login names, None when it names none, names one that is no plain user name, or names the operator, and None too when the operator's home is unknown, so a host never counts the operator's login as an agent's.
    pub fn login_home(login: &str, operator_home: &Path) -> Option<PathBuf> {
        let user = login.split_once('@').map(|(user, _)| user).filter(|user| unix_user(user))?;
        let home = PathBuf::from(format!("/home/{user}"));
        (operator_home.is_absolute() && home != operator_home).then_some(home)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Proxy,
    Lead,
    Worker,
    Reviewer,
}

impl Role {
    pub const ALL: [Self; 4] = [Self::Proxy, Self::Lead, Self::Worker, Self::Reviewer];

    pub fn label(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Lead => "lead",
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
        }
    }

    pub fn parse(text: &str) -> NorthResult<Self> {
        Self::ALL
            .into_iter()
            .find(|role| role.label() == text)
            .ok_or_else(|| NorthError::Usage(format!("no role {text}; use one of {}", Self::ALL.map(Self::label).join(", "))))
    }
}


/// What a launch predicted for a brief that named no files: the files the classifier named, as the write set, and those of them the
/// first prompt carries, as the read set. `missed` is set when the classifier did not answer within its deadline, so the launch went
/// without a prediction.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Predicted {
    pub writes: Vec<String>,
    pub reads: Vec<String>,
    #[serde(default)]
    pub missed: bool,
}

impl Predicted {
    pub fn missed() -> Self {
        Self { missed: true, ..Self::default() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(default)]
    pub verification: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gates_aside: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent: bool,
    /// Its launch declared that its shell tool runs in a task container, not on this host; nothing after the launch sets it.
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
    /// `north fleet dispatch` started it (`--dispatched`), the only kind of session dispatch stops.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dispatched: bool,
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
    pub budget: Option<Stated>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_name: Option<String>,
    /// The files the launch predicted for a brief that named none: the write set the classifier named and the read set the first prompt carried (north#1234).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted: Option<Predicted>,
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
    /// The reserve caller service unit that launched it, read from the launcher's cgroup; only such a session draws a declared reserve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_caller: Option<String>,
    /// The two-rung route `--route` named; only the flag sets it (north#1233).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
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
        self.role.unwrap_or_else(|| match (self.parent.is_some() || self.task_ref.is_some(), self.name == crate::runtime::PROXY) {
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
        self.other_claims.retain(|key| split_key(key).0 != task);
        before != (self.task_ref.clone(), self.extra_tasks.len(), self.other_claims.len())
    }

    /// The keys of the claims this session holds: each task, the first as the box its launch named.
    pub fn claim_keys(&self) -> Vec<String> {
        let tasks: Vec<String> = self.tasks().map(str::to_owned).collect();
        claim_keys(&tasks, self.claim_box.as_deref(), &self.other_claims)
    }

    /// The oldest message to this session's name it takes: its own start, or the start of the session it took the name over from.
    pub fn addressed_since(&self) -> u64 {
        self.addressed_since_ms.unwrap_or(self.created_ms)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LastSender {
    pub name: String,
    pub at_ms: u64,
}

fn unix_user(user: &str) -> bool {
    user.chars().next().is_some_and(|first| first.is_ascii_lowercase() || first == '_') && user.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || matches!(character, '_' | '-'))
}
