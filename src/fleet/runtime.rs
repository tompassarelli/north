use std::ffi::OsString;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::NorthResult;

use super::claude::Claude;
use super::codex::Codex;
use super::model::{Account, LaunchRequest, Provider, Transcript};

pub struct HostSpec {
    pub key: String,
    pub shared: bool,
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub cwd: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub enum Opening<'a> {
    Fresh(Option<&'a str>),
    Resume(&'a str),
}

pub trait Runtime {
    fn assign_conversation(&self) -> NorthResult<Option<String>>;
    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening) -> HostSpec;
    fn handshake(&self) -> Vec<String>;
    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening) -> NorthResult<Vec<String>>;
    fn message(&self, transcript: &Transcript, text: &str) -> NorthResult<Vec<String>>;
    fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>>;
    fn close(&self, transcript: &Transcript) -> Vec<String>;
    fn observe(&self, frame: &Value, transcript: &mut Transcript);
}

pub const DEFAULT_FOREGROUND_CAP_SECS: u64 = 30;

pub fn runtime(provider: Provider) -> &'static dyn Runtime {
    match provider {
        Provider::Claude => &Claude,
        Provider::Codex => &Codex,
    }
}

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

#[cfg(test)]
mod projection {
    use super::*;
    use crate::fleet::model::{EntryKind, Status};

    fn project(provider: Provider, name: &str, fixture: &str) -> Transcript {
        let mut transcript = Transcript::new();
        transcript.name = name.into();
        for line in fixture.lines() {
            runtime(provider).observe(&serde_json::from_str(line).unwrap(), &mut transcript);
        }
        transcript
    }

    fn kinds(transcript: &Transcript) -> Vec<(EntryKind, &str)> {
        transcript.entries.iter().map(|entry| (entry.kind, entry.text.as_str())).collect()
    }

    #[test]
    fn claude_turn_projects_to_user_assistant_and_idle() {
        let transcript = project(Provider::Claude, "t-claude", include_str!("fixtures/claude.jsonl"));
        assert_eq!(
            kinds(&transcript),
            [(EntryKind::User, "Reply with exactly: pong"), (EntryKind::Assistant, "pong"), (EntryKind::TurnEnd, "success")]
        );
        assert_eq!(transcript.status, Status::Idle);
        assert_eq!(transcript.conversation.as_deref(), Some("7557fc31-3bae-471d-8307-ce4fc827601c"));
    }

    #[test]
    fn codex_turn_projects_only_its_own_thread() {
        let transcript = project(Provider::Codex, "t-codex", include_str!("fixtures/codex.jsonl"));
        assert_eq!(
            kinds(&transcript),
            [(EntryKind::User, "Reply with exactly: pong"), (EntryKind::Assistant, "pong"), (EntryKind::TurnEnd, "completed")]
        );
        assert_eq!(transcript.status, Status::Idle);
        assert_eq!(transcript.active_turn, None);
        let stranger = project(Provider::Codex, "other", include_str!("fixtures/codex.jsonl"));
        assert!(stranger.entries.is_empty());
        assert_eq!(stranger.conversation, None);
    }
}
