use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

pub use north_fleet_store::runtime::{PROXY, binary, now_ms};
use super::gates::Question;
use super::model::{Account, Effort, LaunchRequest, ProviderName, Quota, Transcript};
use super::roster::Roster;
use super::turns::TurnSignal;

/// What a profile file holds where a session would run, as a launch check sees it: its text, nothing there, or a machine that did not answer, which the launch itself reports.
pub enum Seen {
    Text(String),
    Absent,
    Unreachable,
}

pub struct HostSpec {
    pub key: String,
    pub shared: bool,
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
}

/// How to start a provider CLI as one account: argv words that read its credential, the environment
/// to set and the inherited variables to drop.
#[derive(Clone, Debug, Default)]
pub struct Invocation {
    pub prefix: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub env_remove: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Instructions<'a> {
    pub text: &'a str,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialFailure {
    SignedOut,
    Expired,
    /// The account's organization turned off subscription access for the client.
    Disabled,
    /// The provider refused the account at its weekly or five-hour limit, whatever its sampled windows read.
    UsageLimit,
}

/// A client's refusal to run on an account, with the text it answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientRefusal {
    pub failure: CredentialFailure,
    pub text: String,
}

pub struct WindowInbox {
    pub account: String,
    pub pid: u32,
    pub socket: PathBuf,
    pub token: String,
    pub busy: bool,
}

pub struct LiveWindow {
    pub conversation: String,
    pub started_ms: u64,
    pub cwd: PathBuf,
    pub inbox: WindowInbox,
}

#[derive(Clone, Copy, Debug)]
pub enum Opening<'a> {
    Fresh(Option<&'a str>),
    Resume(&'a str),
}

pub trait Runtime: Send + Sync {
    fn provider(&self) -> ProviderName;
    fn models(&self) -> &'static [&'static str];
    fn overlay(&self, _name: &str) -> Option<String> {
        None
    }
    fn takes_effort(&self) -> bool {
        false
    }
    fn effort_arms(&self) -> &'static [Effort] {
        &[]
    }
    fn roster(&self, state_root: &Path, home: &Path, policy: &Path) -> Roster;
    fn account_on(&self, _home: &Path, _policy: &Path, _label: &str, machine: &str) -> NorthResult<Account> {
        Err(NorthError::Usage(format!("{} cannot run a session on {machine}", self.provider().label())))
    }
    fn assign_conversation(&self) -> NorthResult<Option<String>>;
    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec>;
    fn interactive(&self, _account: &Account, _request: &LaunchRequest, _conversation: &str, _instructions: &str) -> NorthResult<HostSpec> {
        Err(NorthError::Usage(format!("{} sessions have no interactive UI to hand over to", self.provider().label())))
    }
    /// Writes the first-run state north can complete by itself, so the interactive UI opens on the conversation instead of a setup question; `interactive_prompt` then names whatever would still stop it.
    fn prepare_interactive(&self, _account: &Account, _request: &LaunchRequest) -> NorthResult<()> {
        Ok(())
    }
    fn interactive_prompt(&self, _account: &Account, _request: &LaunchRequest) -> Option<String> {
        None
    }
    fn window_inbox(&self, _accounts: &[Account], _conversation: &str) -> Option<WindowInbox> {
        None
    }
    fn windows(&self, _accounts: &[Account]) -> Vec<LiveWindow> {
        Vec::new()
    }
    fn tell_window(&self, _inbox: &WindowInbox, _text: &str) -> NorthResult<()> {
        Err(NorthError::Usage(format!("{} has no window inbox to deliver into", self.provider().label())))
    }
    /// Prepares the window holding `conversation` to receive reports after `adopt` binds a name to it.
    fn adopt_window(&self, _conversation: &str, _inbox: &WindowInbox) -> NorthResult<()> {
        Ok(())
    }
    /// A `sh` script that copies `conversation`'s transcript from `from`'s home into `to`'s, run where the session runs; `None` when the provider keeps nothing per account.
    fn carry_script(&self, _from: &Account, _to: &Account, _conversation: &str) -> Option<String> {
        None
    }
    fn unguarded(&self, _account: &Account, _read: &dyn Fn(&Path) -> Seen) -> Option<String> {
        None
    }
    fn handshake(&self) -> Vec<String>;
    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening, instructions: Instructions) -> NorthResult<Vec<String>>;
    fn message(&self, transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>>;
    fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>>;
    fn close(&self, transcript: &Transcript) -> Vec<String>;
    fn observe(&self, frame: &Value, transcript: &mut Transcript);
    fn signals(&self, frame: &Value) -> Vec<TurnSignal>;
    fn frame_conversation<'a>(&self, _frame: &'a Value) -> Option<&'a str> { None }
    fn quota(&self, frame: &Value) -> Option<Quota>;
    fn question(&self, _frame: &Value) -> Option<Question> {
        None
    }
    fn credential_failure(&self, _frame: &Value) -> Option<ClientRefusal> {
        None
    }
    /// The tool servers a frame reports as failed to start, so the session runs without the tools they offer.
    fn failed_tool_servers(&self, _frame: &Value) -> Vec<String> {
        Vec::new()
    }
    fn caller_conversation(&self) -> Option<String> {
        None
    }
    fn compact(&self, _focus: Option<&str>) -> Option<String> {
        None
    }
    fn sent_prompt(&self, _frame: &Value) -> Option<String> {
        None
    }
    fn accepted_prompt(&self, _frame: &Value) -> Option<String> {
        None
    }
}


pub const DEFAULT_FOREGROUND_CAP_SECS: u64 = 30;
/// The foreground maximum a session may ask for, which only its hand-in call uses: the hand-in wait and room to print its verdict.
pub const HAND_IN_CAP_SECS: u64 = north_base::land_ticket::HAND_IN_WAIT.as_secs() + 300;
pub const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
pub const ROLE_ENV: &str = "NORTH_FLEET_ROLE";
pub const ACTOR_ENV: &str = "THREADS_ACTOR";
pub const CAPACITY_NEED_ENV: &str = "AGENT_CAPACITY_NEED";

pub fn new_uuid() -> NorthResult<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned())
}

pub fn summarize(text: &str, limit: usize) -> String {
    let line = text.trim();
    match line.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

pub fn labelled_homes(directory: &Path, prefix: &str, marker: &str) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let label = name.strip_prefix(prefix)?.to_owned();
            entry.path().join(marker).is_file().then(|| (label, entry.path()))
        })
        .collect();
    found.sort();
    found
}
