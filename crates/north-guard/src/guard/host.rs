use crate::router::tier::Tier;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub type Pace = fn(&Value) -> Result<Value, String>;
pub type PickTier = fn(&str) -> Result<Tier, String>;
pub type RecordedDecisions = fn(&str, f64) -> Vec<String>;
pub type AccountHome = fn(&str, &str) -> Option<PathBuf>;
pub type FreeCall = fn(&str, &Value) -> bool;
pub type ReadStreakOf = fn(&str, &Value, FreeCall) -> ReadStreak;
pub type StatusVerb = fn(&[&str]) -> bool;
pub type FileToolRefusal = fn(&[String], &Path) -> Option<String>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageReads {
    pub looks: usize,
    pub looking: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReadStreak {
    pub calls: usize,
    pub subject: String,
    pub shapes: Vec<(String, usize)>,
    pub message: MessageReads,
}

#[derive(Clone, Copy)]
pub struct Host {
    pub pace: Pace,
    pub picked_tier: PickTier,
    pub recorded_decisions: RecordedDecisions,
    pub account_home: AccountHome,
    pub read_streak: ReadStreakOf,
    pub status_verb: StatusVerb,
    pub file_tool_refusal: FileToolRefusal,
}

static INSTALLED: OnceLock<Host> = OnceLock::new();

pub fn install(host: Host) {
    let _ = INSTALLED.set(host);
}

fn installed() -> Result<Host, String> {
    INSTALLED.get().copied().ok_or_else(|| "north hook: no guard host installed".to_owned())
}

pub fn pace(request: &Value) -> Result<Value, String> {
    installed().and_then(|host| (host.pace)(request))
}

pub fn picked_tier(brief: &str) -> Result<Tier, String> {
    installed().and_then(|host| (host.picked_tier)(brief))
}

pub fn recorded_decisions(paragraph: &str, now: f64) -> Vec<String> {
    installed().map_or_else(|_| Vec::new(), |host| (host.recorded_decisions)(paragraph, now))
}

pub fn account_home(provider: &str, label: &str) -> Option<PathBuf> {
    installed().ok().and_then(|host| (host.account_home)(provider, label))
}

pub fn read_streak(transcript: &str, hook: &Value) -> Option<ReadStreak> {
    installed().ok().map(|host| (host.read_streak)(transcript, hook, crate::guard::proxy_loads::spends_no_look))
}

pub fn status_verb(words: &[&str]) -> bool {
    installed().is_ok_and(|host| (host.status_verb)(words))
}

/// The first of `paths` a file tool of this session may not write, where this host confines its shell; `None` where no host is installed, which only a test is.
pub fn file_tool_refusal(paths: &[String], cwd: &Path) -> Option<String> {
    installed().ok().and_then(|host| (host.file_tool_refusal)(paths, cwd))
}
