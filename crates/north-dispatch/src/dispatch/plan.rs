//! Where each ready box goes: the cloud lane when it is reachable here and has a free session,
//! else the first host in placement order that serves the box, has a free slot and signs in an
//! account the spend rule admits (docs/rationale/spend-rule.md).
use super::issues::IssueRef;
use super::cloud::{HOST, SESSIONS};
use super::slots::{Account, Charge, Pools};
use super::tier::Route;
use crate::pacing::placement::room::Rank;
use crate::spend::{Refusal, Rules, Starter, View};
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
    pub here: &'a str,
    pub cloud: &'a Cloud,
    pub accounts: &'a [Account],
    /// Expected cost of one box in Max 5x points, charged to its account so the next box ranks the others first.
    pub charge: f64,
    pub spend: &'a View,
    pub rules: &'a Rules,
}

impl Capacity<'_> {
    fn charge(&self, account: &Account) -> Charge {
        Charge { five_hour: self.charge / account.plan.five_hour, week: self.charge / account.plan.week }
    }

    /// The spend rule's verdict on one more worker on `account` after this run's earlier starts.
    fn admits(&self, pools: &Pools, account: &Account) -> Result<(), Refusal> {
        self.spend.admit(self.rules, &account.name, Starter::Worker, pools.placed())
    }
}

/// The shared rank (`room::Rank`) of each account's room less what this run has charged it.
fn rank(pools: &Pools, a: &Account, b: &Account) -> Ordering {
    Rank::of(pools.room(a), a.plan).order(&Rank::of(pools.room(b), b.plan)).then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.admits(pools, account).is_ok()).max_by(|a, b| rank(pools, a, b))
}

/// Why each account signed in on `host` takes no worker, by the spend rule.
fn refused_on(capacity: &Capacity, pools: &Pools, host: &str) -> Vec<String> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0).filter_map(|account| capacity.admits(pools, account).err()).map(|refusal| refusal.to_string()).collect()
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account, local: Option<String>) -> Launch {
    pools.take(host);
    pools.charge(&account.name, capacity.charge(account));
    Launch { item: ready.item.clone(), host: host.into(), account: account.name.clone(), brief: ready.brief.clone(), route: ready.route.clone(), cloud: false, local }
}

/// Admits reviving a session bound to `host` and `account` by the slot and spend rules a launch
/// meets there, taking the slot and its charge.
pub fn revive(capacity: &Capacity, pools: &mut Pools, host: &str, account: &str) -> Result<(), String> {
    let chosen = capacity.accounts.iter().find(|candidate| candidate.name == account).ok_or_else(|| format!("{account} has no usage reading"))?;
    if pools.host(host) == 0 {
        return Err(pools.full(host));
    }
    if pools.pair(host, account) == 0 {
        return Err(format!("{account} is not signed in on {host}"));
    }
    capacity.admits(pools, chosen).map_err(|refusal| refusal.to_string())?;
    pools.take(host);
    pools.charge(account, capacity.charge(chosen));
    Ok(())
}

/// Places one ready box, taking its slot from `pools`, or says why no slot takes it.
pub fn place(ready: &Ready, capacity: &Capacity, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let mut local = None;
    if ready.code_only {
        match capacity.cloud {
            Cloud::Reachable(credit) => {
                if pools.take_cloud() {
                    let account = format!("claude login on {}", capacity.here);
                    return Ok(Launch { item: ready.item.clone(), host: HOST.into(), account, brief: ready.brief.clone(), route: ready.route.clone(), cloud: true, local: None });
                }
                local = Some(format!("north dispatch: cloud lane open ({credit}) but its {SESSIONS} sessions are out"));
            }
            Cloud::Unreachable(why) => local = Some(format!("north dispatch: cloud lane unreachable from {}: {why}", capacity.here)),
            Cloud::Closed(_) => {}
        }
    }
    capacity.spend.fleet(capacity.rules, pools.placed()).map_err(|refusal| refusal.to_string())?;
    let mut refused = Vec::new();
    for candidate in candidates {
        if let Some(why) = &candidate.refusal {
            refused.push(format!("{} {why}", candidate.host));
            continue;
        }
        match best_account(capacity, pools, &candidate.host) {
            Some(account) => return Ok(launch(capacity, pools, ready, &candidate.host, account, local)),
            None if pools.host(&candidate.host) == 0 => refused.push(pools.full(&candidate.host)),
            None => {
                let held = refused_on(capacity, pools, &candidate.host);
                refused.push(if held.is_empty() { format!("{} has no account signed in", candidate.host) } else { format!("{} holds the box: {}", candidate.host, held.join(", ")) });
            }
        }
    }
    Err(if refused.is_empty() { "no host to place it on".into() } else { refused.join("; ") })
}
