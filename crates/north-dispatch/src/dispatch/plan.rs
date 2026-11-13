//! Where each ready box goes: the first host in placement order that serves the box, has a free slot and signs in an
//! account the spend rule admits (docs/rationale/spend-rule.md).
use super::issues::IssueRef;
use super::scope::{Scope, provider_of};
use super::slots::{Account, Charge, Pools};
use super::tier::Route;
use crate::pacing::placement::room::Rank;
use crate::spend::{Refusal, Starter, View};
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub enum Cloud {
    /// The credit is usable and this host reaches the cloud lane.
    Reachable(String),
    /// The credit is usable but this host cannot start cloud sessions, so boxes fall back to hosts.
    Unreachable(String),
    /// No usable credit: code-only boxes run on hosts like any other.
    Closed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub item: IssueRef,
    pub brief: String,
    pub code_only: bool,
    pub route: Route,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub item: IssueRef,
    pub host: String,
    pub account: String,
    pub brief: String,
    pub route: Route,
    /// Started in the cloud lane from this host rather than through `north fleet launch`.
    pub cloud: bool,
    /// Why the box runs here instead of the cloud lane, recorded as `--local`.
    pub local: Option<String>,
}

/// One machine a box may run on, and why it refuses this box: no slots now, or needs it does not
/// serve.
pub struct Candidate {
    pub host: String,
    pub refusal: Option<String>,
}

pub struct Capacity<'a> {
    pub accounts: &'a [Account],
    /// Expected cost of one box in Max 5x points, charged to its account so the next box ranks the others first.
    pub charge: f64,
    /// Percent of a window used at which an account takes no dispatched box (`money_ceiling`).
    pub ceiling: f64,
    /// Dispatched Claude sessions that may work at once (`dispatch_claude_sessions`).
    pub claude_sessions: usize,
    pub spend: &'a View,
    pub scope: &'a Scope,
}

impl Capacity<'_> {
    fn charge(&self, account: &Account) -> Charge {
        Charge { five_hour: self.charge / account.plan.five_hour, week: self.charge / account.plan.week }
    }

    /// Whether `account` takes one more worker: not the operational reserve, not at its provider's limit and under the money ceiling.
    fn admits(&self, account: &Account) -> Result<(), Refusal> {
        self.spend.admit(&account.name, Starter::Worker)?;
        self.spend.under_ceiling(&account.name, self.ceiling)
    }
}

/// The shared rank (`room::Rank`) of each account's room less what this run has charged it.
fn rank(pools: &Pools, a: &Account, b: &Account) -> Ordering {
    let ranked = |account: &Account| account.grant.unwrap_or_else(|| Rank::of(pools.room(account), account.plan));
    ranked(a).order(&ranked(b)).then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.admits(account).is_ok()).max_by(|a, b| rank(pools, a, b))
}

/// Why each account signed in on `host` takes no worker, by the spend rule.
fn refused_on(capacity: &Capacity, pools: &Pools, host: &str) -> Vec<String> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0).filter_map(|account| capacity.admits(account).err()).map(|refusal| refusal.to_string()).collect()
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account) -> Launch {
    pools.take(host);
    pools.take_claude();
    pools.charge(&account.name, capacity.charge(account));
    Launch { item: ready.item.clone(), host: host.into(), account: account.name.clone(), brief: ready.brief.clone(), route: ready.route.clone(), cloud: false, local: None }
}

/// Admits reviving a session bound to `host` and `account` by the slot and spend rules a launch
/// meets there, taking the slot and its charge.
pub fn revive(capacity: &Capacity, pools: &mut Pools, host: &str, account: &str) -> Result<(), String> {
    if capacity.scope.excludes_host(host) {
        return Err(capacity.scope.why_not_host(host));
    }
    if !capacity.scope.allows_provider(provider_of(account)) {
        return Err(capacity.scope.why_not_provider(provider_of(account)));
    }
    let chosen = capacity.accounts.iter().find(|candidate| candidate.name == account).ok_or_else(|| format!("{account} has no usage reading"))?;
    if pools.host(host) == 0 {
        return Err(pools.full(host));
    }
    if pools.pair(host, account) == 0 {
        return Err(format!("{account} is not signed in on {host}"));
    }
    capacity.admits(chosen).map_err(|refusal| refusal.to_string())?;
    pools.take(host);
    pools.charge(account, capacity.charge(chosen));
    Ok(())
}

/// Places one ready box, taking its slot from `pools`, or says why no slot takes it.
pub fn place(ready: &Ready, capacity: &Capacity, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let working = capacity.spend.working + pools.claude_placed();
    if working >= capacity.claude_sessions {
        return Err(format!("{working} Claude sessions are working, at the dispatch_claude_sessions cap of {}", capacity.claude_sessions));
    }
    let mut refused = Vec::new();
    for candidate in candidates {
        if let Some(why) = &candidate.refusal {
            refused.push(format!("{} {why}", candidate.host));
            continue;
        }
        match best_account(capacity, pools, &candidate.host) {
            Some(account) => return Ok(launch(capacity, pools, ready, &candidate.host, account)),
            None if pools.host(&candidate.host) == 0 => refused.push(pools.full(&candidate.host)),
            None => {
                let held = refused_on(capacity, pools, &candidate.host);
                refused.push(if held.is_empty() { format!("{} has no account signed in", candidate.host) } else { format!("{} holds the box: {}", candidate.host, held.join(", ")) });
            }
        }
    }
    Err(if refused.is_empty() { "no host to place it on".into() } else { refused.join("; ") })
}
