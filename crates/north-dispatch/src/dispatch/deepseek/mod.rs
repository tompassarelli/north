//! DeepSeek staffing (north#1020): dispatch puts a ready box on an opencode worker ahead of every Claude account while the
//! opencode cap in harness/opencode.toml has room, the box's subject may leave Claude and DeepSeek has not tried that box;
//! a box DeepSeek tried goes to Claude. A DeepSeek session that reports `Done:` queues one review job on its host
//! (`review`), which samples its commit for a Sonnet review (`sampling`) or lands it through safe-push, and forgets the
//! session so its slot frees.
use super::issues::{Issue, IssueRef};
use super::plan::{Candidate, Launch, Ready};
use super::record::Record;
use super::fleets::{self, PROBE, Session};
use super::slots::Pools;
use regex::Regex;
use serde_json::Value;
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

/// The rung a launch line names: `deepseek`, or its Claude tier.
pub fn rung(launch: &Launch) -> &'static str {
    if is(launch) { "deepseek" } else { launch.route.tier.name() }
}

/// The model words `north fleet launch` takes for `launch`: none for DeepSeek, whose launcher fixes the model.
pub fn invocation(launch: &Launch, cfg: &DocumentMut) -> Result<Vec<String>, String> {
    if is(launch) { Ok(Vec::new()) } else { launch.route.invocation(cfg) }
}

/// The Claude tier a launch record keeps, which the climb reads as a rung tried; a DeepSeek launch tried none.
pub fn tier(launch: &Launch) -> Option<String> {
    (!is(launch)).then(|| launch.route.tier.name().to_owned())
}

/// The routing cell a launch record keeps; a DeepSeek launch ran on none of Claude's.
pub fn cell(launch: &Launch, cfg: &DocumentMut) -> Option<String> {
    let (model, effort) = launch.route.model_effort(cfg).ok().filter(|_| !is(launch))?;
    super::cells::named(&model, &effort)
}

impl Seen {
    /// What dispatch reads about DeepSeek: the opencode sessions holding a slot across every listing, the boxes the dispatch
    /// record shows it tried, and the idle sessions on its hosts whose last report starts `Done:`.
    #[allow(clippy::too_many_arguments, reason = "each input is a reading the dispatch survey already holds")]
    pub fn read(here: &str, laptop: Option<&str>, fleet_lists: &[(String, Value)], laptop_reports: &Value, sessions: &[(Session, String)], records: &[Record], issues: &[Issue], at: f64, notes: &mut Vec<String>) -> Self {
        let settings = Settings::read();
        let holding = sessions.iter().filter(|(session, _)| session.account.as_deref() == Some(PROVIDER)).map(|(session, _)| session.name.as_str()).collect::<BTreeSet<_>>().len();
        let mut reports: Vec<(String, Value)> = laptop.filter(|host| settings.hosts.iter().any(|name| name == host)).map(|host| (host.to_owned(), laptop_reports.clone())).into_iter().collect();
        if settings.hosts.iter().any(|name| name == here) {
            match fleets::remote_json_within(None, &["fleet", "reports", "--json"], PROBE) {
                Ok(listing) => reports.push((here.to_owned(), listing)),
                Err(why) => notes.push(format!("{here} fleet reports unread, so no DeepSeek review is queued here this run: {why}")),
            }
        }
        let queued = review::queued(&super::record::directory(), at);
        let done = review::done(fleet_lists, &reports, records, issues, &queued);
        Self { holding: i64::try_from(holding).unwrap_or(i64::MAX), tried: tried(records), done, settings }
    }

    /// Places `ready` on DeepSeek when it takes the box, counting it in `placed`, else on Claude through `claude`.
    #[allow(clippy::too_many_arguments, reason = "the dispatch loop's own state for one box")]
    pub fn staff(&self, issue: &Issue, box_text: Option<&str>, ready: &Ready, candidates: &[Candidate], pools: &mut Pools, placed: &mut i64, claude: impl FnOnce(&mut Pools) -> Result<Launch, String>) -> Result<Launch, String> {
        match self.takes(issue, box_text, *placed).and_then(|()| self.place(ready, candidates, pools)) {
            Ok(launch) => {
                *placed += 1;
                Ok(launch)
            }
            Err(_) => claude(pools),
        }
    }

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
        let route = super::tier::Route { effort: None, effort_arm: false, signal: None, ..ready.route.clone() };
        let local = Some(format!("north fleet dispatch: DeepSeek takes this box on {host}, the host that holds the opencode key (north#1020)"));
        Ok(Launch { item: ready.item.clone(), host: host.clone(), account: ACCOUNT.into(), brief: ready.brief.clone(), route, cloud: false, local })
    }
}
