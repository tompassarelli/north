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
pub mod limit;
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
    pub review: review::Found,
    /// The opencode sessions running on every host now.
    pub running: usize,
    /// The Go plan's limit, read from the sessions it stopped.
    pub limit: limit::Limit,
    /// Spend's provider-limit refusal while that limit holds, which places no opencode box.
    pub held: Option<crate::spend::Refusal>,
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

/// The `fleet reports` of every host whose listing holds a session of the provider waiting for its review job: the laptop's
/// from its mirrored copy, this host's by running it, each other host's through `read` over ssh. The host that holds a
/// session may be none of the provider's hosts, since a session placed on a build host is listed by the host that launched it.
pub fn reports(here: &str, laptop: Option<&str>, laptop_reports: &Value, lists: &[(String, Value)], marked: &review::Marked, read: &dyn Fn(Option<&str>) -> Result<Value, String>, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let mut found: Vec<(String, Value)> = Vec::new();
    for (host, _) in lists.iter().filter(|(_, listing)| review::has_waiting(listing, marked)) {
        if laptop == Some(host.as_str()) {
            found.push((host.clone(), laptop_reports.clone()));
            continue;
        }
        match read((host != here).then_some(host.as_str())) {
            Ok(listing) => found.push((host.clone(), listing)),
            Err(why) => notes.push(format!("{host} fleet reports unread, so no DeepSeek review is queued there this run: {why}")),
        }
    }
    found
}

/// Each listing of `lists` but the laptop's read again with `--all`, so it also shows the sessions the fleet retired.
fn with_retired(cfg: &DocumentMut, here: &str, laptop: Option<&str>, lists: &[(String, Value)], notes: &mut Vec<String>) -> Vec<(String, Value)> {
    lists
        .iter()
        .map(|(host, listing)| {
            if laptop == Some(host.as_str()) {
                return (host.clone(), listing.clone());
            }
            match fleets::remote_json_within((host != here).then(|| fleets::destination(cfg, host)).as_deref(), &["fleet", "list", "--json", "--all"], PROBE) {
                Ok(all) => (host.clone(), all),
                Err(why) => {
                    notes.push(format!("{host} fleet list --all unread, so no retired DeepSeek session there is reviewed this run: {why}"));
                    (host.clone(), listing.clone())
                }
            }
        })
        .collect()
}

/// The provider's sessions that count against its slot cap: those running or resting, less the resting ones whose review job is queued, since that job forgets them, and those its limit stopped, which dispatch forgets.
pub fn counted(lists: &[(String, Value)], queued: &BTreeSet<String>) -> usize {
    let parked: BTreeSet<&str> = lists.iter().flat_map(|(_, listing)| listing.as_array().into_iter().flatten()).filter(|session| limit::parked(session)).filter_map(|session| session["name"].as_str()).collect();
    lists
        .iter()
        .flat_map(|(_, listing)| fleets::running_sessions(listing))
        .filter(|session| session.account.as_deref() == Some(PROVIDER) && (session.working || !queued.contains(&session.name)) && !parked.contains(session.name.as_str()))
        .map(|session| session.name)
        .collect::<BTreeSet<_>>()
        .len()
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

/// The Claude tier a launch record keeps, read as a rung tried; a DeepSeek launch tried none.
pub fn tier(launch: &Launch) -> Option<String> {
    (!is(launch)).then(|| launch.route.tier.name().to_owned())
}

/// The routing cell a launch record keeps; a DeepSeek launch ran on none of Claude's.
pub fn cell(launch: &Launch, cfg: &DocumentMut) -> Option<String> {
    let (model, effort) = launch.route.model_effort(cfg).ok().filter(|_| !is(launch))?;
    super::cells::named(&model, &effort)
}

impl Seen {
    /// What dispatch reads about DeepSeek: the boxes the dispatch record shows it tried, and the sessions of the
    /// provider on every host, resting or retired unreviewed, whose last report starts `Done:`.
    #[allow(clippy::too_many_arguments, reason = "each input is a reading the dispatch survey already holds")]
    pub fn read(cfg: &DocumentMut, here: &str, laptop: Option<&str>, fleet_lists: &[(String, Value)], laptop_reports: &Value, records: &[Record], issues: &[Issue], at: f64, notes: &mut Vec<String>) -> Self {
        let settings = Settings::read();
        let read = |host: Option<&str>| fleets::remote_json_within(host.map(|name| fleets::destination(cfg, name)).as_deref(), &["fleet", "reports", "--json"], PROBE);
        let marked = review::queued(&super::record::directory(), at);
        let lists = with_retired(cfg, here, laptop, fleet_lists, notes);
        let reports = reports(here, laptop, laptop_reports, &lists, &marked, &read, notes);
        let done = review::done(&lists, &reports, records, issues, &marked);
        let limit = limit::Limit::read(fleet_lists, &super::record::directory(), at);
        let held = limit.refusal(at);
        notes.extend(held.as_ref().map(|refusal| format!("DeepSeek places no box: {refusal}")));
        Self { tried: tried(records), review: done, running: counted(fleet_lists, &marked.recent), settings, limit, held }
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
        if let Some(refusal) = &self.held {
            return Err(refusal.to_string());
        }
        cap::admit(self.running + pools.deepseek_placed(), self.settings.slot_cap)?;
        let serves = |host: &str| candidates.iter().any(|candidate| candidate.host == host && candidate.refusal.is_none());
        let none = || match pools.line_held() {
            Some(why) => format!("no DeepSeek host takes this box: {why}"),
            None => format!("no DeepSeek host ({}) serves this box with a free slot", self.settings.hosts.join(", ")),
        };
        let host = self.settings.hosts.iter().find(|host| serves(host) && pools.host(host) > 0).ok_or_else(none)?;
        pools.take(host);
        pools.take_deepseek();
        let route = super::tier::Route { effort: None, effort_arm: false, signal: None, cell: None, cell_role: None, ..ready.route.clone() };
        let local = Some(format!("north fleet dispatch: DeepSeek takes this box on {host}, the host that holds the opencode key (north#1020)"));
        Ok(Launch { item: ready.item.clone(), host: host.clone(), account: ACCOUNT.into(), brief: format!("{}\n\n{}", ready.brief, review::worker_order()), route, local, lineage: super::plan::Lineage::default() })
    }
}
