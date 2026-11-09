//! DeepSeek staffing (north#1020): dispatch puts a ready box on an opencode worker ahead of every Claude account while the
//! opencode cap in harness/opencode.toml has room, the box's subject may leave Claude and DeepSeek has not tried that box;
//! a box DeepSeek tried goes to Claude. A DeepSeek session that reports `Done:` queues one review job on its host
//! (`review`), which samples its commit for a Sonnet review (`sampling`) or lands it through safe-push, and forgets the
//! session so its slot frees.
use super::issues::{Issue, IssueRef};
use super::plan::{Candidate, Launch, Ready};
use super::record::Record;
use super::slots::Pools;
use regex::Regex;
use std::collections::BTreeSet;
use toml_edit::{DocumentMut, Item};

pub mod review;
pub mod sampling;
#[cfg(test)]
mod tests;

pub const ACCOUNT: &str = "opencode:go";
pub const PROVIDER: &str = "opencode";
const SETTINGS: &str = "harness/opencode.toml";

#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// DeepSeek workers that may hold a session at once, apart from the Claude worker cap.
    pub slots: i64,
    /// The hosts that hold the opencode key, in the order dispatch fills them.
    pub hosts: Vec<String>,
    /// Subjects whose boxes stay on Claude, matched against the issue title and the box.
    pub claude_only: Vec<Regex>,
}

impl Settings {
    pub fn read() -> Self {
        Self::parse(crate::platform::harness::document(SETTINGS))
    }

    pub fn parse(document: &DocumentMut) -> Self {
        let strings = |item: Option<&Item>| -> Vec<String> { item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect() };
        let dispatch = document.get("dispatch");
        Self {
            slots: document.get("go").and_then(|go| go.get("slots")).and_then(Item::as_integer).unwrap_or(0),
            hosts: strings(dispatch.and_then(|table| table.get("hosts"))),
            claude_only: strings(dispatch.and_then(|table| table.get("claude_only"))).iter().filter_map(|pattern| Regex::new(&format!("(?i){pattern}")).ok()).collect(),
        }
    }
}

/// What a dispatch run reads about DeepSeek: its settings, the opencode sessions holding a slot, the boxes it tried and
/// the sessions that reported Done.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub settings: Settings,
    pub holding: i64,
    pub tried: BTreeSet<String>,
    pub done: Vec<review::Done>,
}

/// One box of one item, as `tried` keys it.
pub fn key(item: &IssueRef, box_text: Option<&str>) -> String {
    format!("{item}\t{}", box_text.unwrap_or_default())
}

/// The boxes DeepSeek was launched on, from the dispatch record.
pub fn tried(records: &[Record]) -> BTreeSet<String> {
    records.iter().filter(|record| record.launched && record.account == ACCOUNT).filter_map(|record| Some(key(&IssueRef::parse(&record.item)?, record.box_text.as_deref()))).collect()
}

pub fn is(launch: &Launch) -> bool {
    launch.account == ACCOUNT
}

impl Seen {
    /// Why `issue`'s box stays on Claude, or `Ok` when DeepSeek may take it.
    pub fn takes(&self, issue: &Issue, box_text: Option<&str>, placed: i64) -> Result<(), String> {
        if self.holding + placed >= self.settings.slots {
            return Err(format!("the opencode cap of {} is full", self.settings.slots));
        }
        if self.tried.contains(&key(&issue.reference(), box_text)) {
            return Err("DeepSeek already tried this box".into());
        }
        let subject = format!("{}\n{}", issue.title, box_text.unwrap_or_default());
        if let Some(pattern) = self.settings.claude_only.iter().find(|pattern| pattern.is_match(&subject)) {
            return Err(format!("its subject matches {}, which stays on Claude", pattern.as_str().trim_start_matches("(?i)")));
        }
        Ok(())
    }

    /// Places `ready` on the first DeepSeek host that serves the box and has a free slot, taking that slot, or says why none takes it.
    pub fn place(&self, ready: &Ready, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
        let serves = |host: &str| candidates.iter().any(|candidate| candidate.host == host && candidate.refusal.is_none());
        let host = self.settings.hosts.iter().find(|host| serves(host) && pools.host(host) > 0).ok_or_else(|| format!("no DeepSeek host ({}) serves this box with a free slot", self.settings.hosts.join(", ")))?;
        pools.take_slot(host);
        let local = Some(format!("north fleet dispatch: DeepSeek takes this box on {host}, the host that holds the opencode key (north#1020)"));
        Ok(Launch { item: ready.item.clone(), host: host.clone(), account: ACCOUNT.into(), brief: ready.brief.clone(), route: ready.route.clone(), cloud: false, local })
    }
}
