use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::gates::Question;
use super::model::{Account, LaunchRequest, ProviderName, Quota, Transcript};
use super::roster::Roster;
use super::turns::TurnSignal;

pub struct HostSpec {
    pub key: String,
    pub shared: bool,
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub cwd: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub struct Instructions<'a> {
    pub text: &'a str,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Opening<'a> {
    Fresh(Option<&'a str>),
    Resume(&'a str),
}

pub trait Runtime: Send + Sync {
    fn provider(&self) -> ProviderName;
    fn models(&self) -> &'static [&'static str];
    fn roster(&self, state_root: &Path, home: &Path) -> Roster;
    fn assign_conversation(&self) -> NorthResult<Option<String>>;
    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec>;
    fn handshake(&self) -> Vec<String>;
    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening, instructions: Instructions) -> NorthResult<Vec<String>>;
    fn message(&self, transcript: &Transcript, text: &str) -> NorthResult<Vec<String>>;
    fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>>;
    fn close(&self, transcript: &Transcript) -> Vec<String>;
    fn observe(&self, frame: &Value, transcript: &mut Transcript);
    fn signals(&self, frame: &Value) -> Vec<TurnSignal>;
    fn frame_conversation<'a>(&self, _frame: &'a Value) -> Option<&'a str> { None }
    fn quota(&self, frame: &Value) -> Option<Quota>;
    fn question(&self, _frame: &Value) -> Option<Question> {
        None
    }
    fn signed_out(&self, _frame: &Value) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct Runtimes(Vec<Arc<dyn Runtime>>);

impl Runtimes {
    pub fn new(runtimes: Vec<Arc<dyn Runtime>>) -> Self {
        Self(runtimes)
    }

    pub fn get(&self, provider: &ProviderName) -> NorthResult<Arc<dyn Runtime>> {
        self.0
            .iter()
            .find(|runtime| &runtime.provider() == provider)
            .cloned()
            .ok_or_else(|| NorthError::Usage(format!("no runtime for provider {}", provider.label())))
    }

    pub fn providers(&self) -> Vec<ProviderName> {
        self.0.iter().map(|runtime| runtime.provider()).collect()
    }

    pub fn roster(&self, state_root: &Path, home: &Path) -> Roster {
        self.0.iter().map(|runtime| runtime.roster(state_root, home)).fold(Roster::default(), Roster::merge)
    }
}

pub const DEFAULT_FOREGROUND_CAP_SECS: u64 = 30;
pub const SESSION_ENV: &str = "NORTH_FLEET_SESSION";

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

pub fn binary(name: &str) -> PathBuf {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file());
    let local = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin").join(name));
    on_path.or(local).unwrap_or_else(|| PathBuf::from(name))
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

pub fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().unix_millis()).unwrap_or(0)
}
