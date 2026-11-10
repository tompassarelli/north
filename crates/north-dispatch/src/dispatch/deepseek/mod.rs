//! DeepSeek staffing (north#1020): dispatch puts a ready box on an opencode worker ahead of every Claude account while the
//! box's subject may leave Claude and DeepSeek has not tried that box; a box DeepSeek tried goes to Claude. A DeepSeek
//! session that reports `Done:` queues one review job on its host (`review`), which samples its commit for a Sonnet review
//! (`sampling`) or lands it through safe-push, and forgets the session so its slot frees.
use super::issues::{Issue, IssueRef};
use super::plan::{Candidate, Launch, Ready};
use super::record::Record;
use super::fleets::{self, PROBE};
use super::shares::DEEPSEEK_ARMS;
use super::slots::Pools;
use super::tier::{Effort, Evidence, History};
use crate::fleet::claude_only::{ClaudeOnly, SETTINGS};
use serde_json::Value;
use std::collections::BTreeSet;
use toml_edit::{DocumentMut, Item};

pub mod cap;
pub mod mechanical;
pub mod review;
pub mod sampling;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod mechanical_tests;
#[cfg(test)]
mod effort_tests;

pub const ACCOUNT: &str = "opencode:go";
pub const PROVIDER: &str = "opencode";

#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// The hosts that hold the opencode key, in the order dispatch fills them.
    pub hosts: Vec<String>,
    /// Subjects whose boxes stay on Claude, matched against the issue title and the box.
    pub claude_only: ClaudeOnly,
    /// How many opencode workers run at once, apart from the Claude workers.
    pub slot_cap: usize,
}

impl Settings {
    pub fn read() -> Self {
        Self::parse(crate::platform::harness::document(SETTINGS))
    }

    pub fn parse(document: &DocumentMut) -> Self {
        let strings = |item: Option<&Item>| -> Vec<String> { item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect() };
        let dispatch = document.get("dispatch");
        Self {
            hosts: strings(dispatch.and_then(|table| table.get("hosts"))),
            claude_only: ClaudeOnly::parse(document),
            slot_cap: dispatch.and_then(|table| table.get("slot_cap")).and_then(Item::as_integer).map_or(0, |cap| cap.max(0) as usize),
        }
    }
}

/// What a dispatch run reads about DeepSeek: its settings, the boxes it tried and the sessions that reported Done.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub settings: Settings,
    pub tried: BTreeSet<String>,
    pub done: Vec<review::Done>,
    /// The opencode sessions running on every host now.
    pub running: usize,
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

/// The words `north fleet launch` takes for `launch`: for DeepSeek, whose launcher fixes the model, only the effort its arm drew.
pub fn invocation(launch: &Launch, cfg: &DocumentMut) -> Result<Vec<String>, String> {
    if is(launch) { Ok(launch.route.effort.map(|effort| vec!["--effort".to_owned(), effort.name().to_owned()]).unwrap_or_default()) } else { launch.route.invocation(cfg) }
}

/// The reasoning effort this launch draws from its category's DeepSeek shares, the `Arm:` stamp that records it and the words naming the share it drew from.
pub fn effort_arm(item: &str, category: &str, history: &History, evidence: &Evidence) -> Option<(Effort, &'static str, String)> {
    let shares = evidence.shares(category, "deepseek");
    let (arm, drawn) = shares.draw(&history.seed("deepseek", item));
    let stamp = DEEPSEEK_ARMS.into_iter().find(|stamp| *stamp == arm)?;
    Some((Effort::parse(stamp.strip_prefix("deepseek-")?)?, stamp, drawn))
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
    /// What dispatch reads about DeepSeek: the boxes the dispatch record shows it tried, and the idle sessions on its
    /// hosts whose last report starts `Done:`.
    #[allow(clippy::too_many_arguments, reason = "each input is a reading the dispatch survey already holds")]
    pub fn read(here: &str, laptop: Option<&str>, fleet_lists: &[(String, Value)], laptop_reports: &Value, records: &[Record], issues: &[Issue], at: f64, notes: &mut Vec<String>) -> Self {
        let settings = Settings::read();
        let mut reports: Vec<(String, Value)> = laptop.filter(|host| settings.hosts.iter().any(|name| name == host)).map(|host| (host.to_owned(), laptop_reports.clone())).into_iter().collect();
        if settings.hosts.iter().any(|name| name == here) {
            match fleets::remote_json_within(None, &["fleet", "reports", "--json"], PROBE) {
                Ok(listing) => reports.push((here.to_owned(), listing)),
                Err(why) => notes.push(format!("{here} fleet reports unread, so no DeepSeek review is queued here this run: {why}")),
            }
        }
        let queued = review::queued(&super::record::directory(), at);
        let done = review::done(fleet_lists, &reports, records, issues, &queued);
        let names: BTreeSet<String> = fleet_lists.iter().flat_map(|(_, listing)| super::fleets::running_sessions(listing)).filter(|session| session.account.as_deref() == Some(PROVIDER)).map(|session| session.name).collect();
        Self { tried: tried(records), done, running: names.len(), settings }
    }

    /// Places `ready` on DeepSeek when it takes the box, else on Claude through `claude`.
    pub fn staff(&self, issue: &Issue, box_text: Option<&str>, ready: &Ready, candidates: &[Candidate], pools: &mut Pools, claude: impl FnOnce(&mut Pools) -> Result<Launch, String>) -> Result<Launch, String> {
        match self.takes(issue, box_text).and_then(|()| self.place(ready, candidates, pools)) {
            Ok(launch) => Ok(launch),
            Err(_) => claude(pools),
        }
    }

    /// Why `issue`'s box stays on Claude, or `Ok` when DeepSeek may take it.
    pub fn takes(&self, issue: &Issue, box_text: Option<&str>) -> Result<(), String> {
        if self.tried.contains(&key(&issue.reference(), box_text)) {
            return Err("DeepSeek already tried this box".into());
        }
        if let Some(pattern) = self.settings.claude_only.matching(&issue.title, box_text) {
            return Err(format!("its subject matches {pattern}, which stays on Claude"));
        }
        Ok(())
    }

    /// Places `ready` on the first DeepSeek host that serves the box and has a free slot, taking that slot, or says why none takes it.
    pub fn place(&self, ready: &Ready, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
        cap::admit(self.running + pools.deepseek_placed(), self.settings.slot_cap)?;
        let serves = |host: &str| candidates.iter().any(|candidate| candidate.host == host && candidate.refusal.is_none());
        let host = self.settings.hosts.iter().find(|host| serves(host) && pools.host(host) > 0).ok_or_else(|| format!("no DeepSeek host ({}) serves this box with a free slot", self.settings.hosts.join(", ")))?;
        pools.take_slot(host);
        pools.take_deepseek();
        let route = super::tier::Route { effort: None, effort_arm: false, signal: None, ..ready.route.clone() };
        let local = Some(format!("north fleet dispatch: DeepSeek takes this box on {host}, the host that holds the opencode key (north#1020)"));
        Ok(Launch { item: ready.item.clone(), host: host.clone(), account: ACCOUNT.into(), brief: format!("{}\n\n{}", ready.brief, review::worker_order()), route, cloud: false, local })
    }
}
