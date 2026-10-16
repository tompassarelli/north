use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::fleet::books::Digest;
use crate::fleet::gates::Gate;
use crate::fleet::model::{Account, EntryKind, Quota, SessionRecord, Transcript};
use crate::fleet::registry::urgent_text;

use super::desk::SessionSummary;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFacts {
    pub task: Option<String>,
    pub report: Option<String>,
    pub live_tokens: u64,
    pub quota: Option<Quota>,
}

impl SessionFacts {
    pub fn of(record: &SessionRecord, transcript: Option<&Transcript>) -> Self {
        let entries = transcript.map_or(&[][..], |transcript| transcript.entries.as_slice());
        let first_prompt = entries.iter().filter(|entry| entry.kind == EntryKind::User)
            .map(|entry| urgent_text(&entry.text).unwrap_or(&entry.text))
            .find(|prompt| !prompt.trim_start().starts_with("<task-notification>"))
            .map(task_title);
        let finished = entries.iter().rposition(|entry| entry.kind == EntryKind::TurnEnd).unwrap_or(0);
        let report = entries[..finished].iter().rev().find(|entry| entry.kind == EntryKind::Assistant).map(|entry| first_line(&entry.text));
        Self {
            task: record.task_ref.clone().or(first_prompt),
            report,
            live_tokens: transcript.map_or(0, |transcript| transcript.tracker.live_tokens()),
            quota: transcript.and_then(|transcript| transcript.quota.clone()) }
    }
}

#[derive(Default)]
pub struct DigestPane {
    pub shown: bool,
    pub wisp_payoff: String,
    pub books: Digest,
    pub facts: BTreeMap<String, SessionFacts>,
    pub selected_gate: Option<i64>,
    seen_gates: BTreeSet<i64>,
}

impl DigestPane {
    pub fn update(&mut self, books: Digest) -> Vec<String> {
        let arrived: Vec<String> = books.open_gates().filter(|gate| self.seen_gates.insert(gate.id)).map(|gate| gate.request.session.clone()).collect();
        self.books = books;
        if self.selected().is_none() {
            self.selected_gate = self.books.open_gates().next().map(|gate| gate.id);
        }
        arrived
    }

    pub fn selected(&self) -> Option<&Gate> {
        let id = self.selected_gate?;
        self.books.open_gates().find(|gate| gate.id == id)
    }

    pub fn quota(&self, account: &Account, sessions: &[SessionSummary]) -> Option<Quota> {
        sessions
            .iter()
            .filter(|session| session.record.provider == account.provider && session.record.account == account.label)
            .filter_map(|session| Some((session.last_activity_ms, self.facts.get(&session.record.name)?.quota.clone()?)))
            .max_by_key(|(seen, _)| *seen)
            .map(|(_, quota)| quota)
    }

    pub fn move_selection(&mut self, delta: isize) {
        let open: Vec<i64> = self.books.open_gates().map(|gate| gate.id).collect();
        let current = self.selected_gate.and_then(|id| open.iter().position(|open| *open == id)).unwrap_or(0);
        self.selected_gate = open.get(current.saturating_add_signed(delta).min(open.len().saturating_sub(1))).copied();
    }
}

fn task_title(prompt: &str) -> String {
    brief_path(prompt).and_then(|path| title_of(&path)).unwrap_or_else(|| first_line(prompt))
}

fn brief_path(prompt: &str) -> Option<PathBuf> {
    prompt
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| matches!(character, '"' | '\'' | '`' | '.' | ',' | ':' | ')')))
        .find(|word| word.starts_with('/') && word.ends_with(".md"))
        .map(PathBuf::from)
}

fn title_of(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    BufReader::new(file).lines().map_while(Result::ok).map(|line| line.trim().trim_start_matches('#').trim().to_owned()).find(|line| !line.is_empty())
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_owned()
}
