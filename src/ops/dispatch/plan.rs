//! Where each ready box goes: the cloud lane when it is reachable here and has a free session,
//! else the first host in placement order that serves the box and has a free slot.
use super::issues::IssueRef;
use super::cloud::{HOST, SESSIONS};
use super::slots::{Account, Charge, Pools};
use crate::ops::pacing::placement::room::Rank;
use super::tier::Route;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cloud {
    /// The credit is usable and this host reaches the cloud lane.
    Reachable(String),
    /// The credit is usable but this host cannot start cloud sessions, so boxes fall back to hosts.
    Unreachable(String),
    /// No usable credit: code-only boxes run on hosts like any other.
    Closed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ready {
    pub item: IssueRef,
    pub brief: String,
    pub code_only: bool,
    pub route: Route,
    /// Hours the ledger expects the box to run, when its cell has closed boxes.
    pub eta_h: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Launch {
    pub item: IssueRef,
    pub host: String,
    pub account: String,
    pub brief: String,
    pub route: Route,
    /// Started in the cloud lane from this host rather than through `north fleet launch`.
    pub cloud: bool,
    /// Why the box runs here instead of the cloud lane, recorded as `--local`.
    pub local: Option<String>,
    /// Why the account is not the one placement would pick, recorded as `--pin`.
    pub pin: Option<String>,
    pub handoff: Option<String>,
    /// The measured burn the account was placed by, when it has one.
    pub burn: Option<String>,
}

/// One machine a box may run on, and why it refuses this box: no slots now, or needs it does not
/// serve.
pub(crate) struct Candidate {
    pub host: String,
    pub refusal: Option<String>,
}

pub(crate) struct Capacity<'a> {
    pub here: &'a str,
    pub cloud: &'a Cloud,
    pub accounts: &'a [Account],
    /// Expected cost of one box in Max 5x points, charged to its account before the next box is ranked.
    pub charge: f64,
}

impl Capacity<'_> {
    fn charge(&self, account: &Account) -> Charge {
        Charge { five_hour: self.charge / account.plan.five_hour, week: self.charge / account.plan.week }
    }

    /// Whether the account can take one more box this run without crossing a window's stop, and
    /// without its measured burn over the box's ETA crossing the five-hour stop.
    fn takes(&self, pools: &Pools, account: &Account, eta_h: Option<f64>) -> bool {
        let charge = self.charge(account);
        pools.room(account).takes(charge) && pools.room(account).less(charge).five_hour > burned(account, eta_h).unwrap_or(0.0)
    }
}

/// Points of its own percent the account's measured burn spends over `eta_h` hours.
fn burned(account: &Account, eta_h: Option<f64>) -> Option<f64> {
    Some(account.burn? * eta_h?)
}

/// What the account's measured burn and the box's ETA say, for the launch log.
fn burn_line(pools: &Pools, account: &Account, eta_h: Option<f64>) -> Option<String> {
    let (burn, burned) = (account.burn?, burned(account, eta_h)?);
    Some(format!("{} measured burn {burn:.1}%/h over a {:.0} min ETA spends {burned:.1} of the {:.0} points left to its 5h stop", account.name, eta_h? * 60.0, pools.room(account).five_hour))
}

/// The accounts signed in on `host` that would take the box but for their measured burn, each with
/// the points that burn spends over the ETA.
fn burn_held(capacity: &Capacity, pools: &Pools, host: &str, eta_h: Option<f64>) -> Vec<String> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.takes(pools, account, None) && !capacity.takes(pools, account, eta_h)).filter_map(|account| burn_line(pools, account, eta_h)).collect()
}

/// The shared rank (`room::Rank`) of each account's room less what this run has charged it.
fn rank(pools: &Pools, a: &Account, b: &Account) -> Ordering {
    Rank::of(pools.room(a), a.plan, a.spending).order(&Rank::of(pools.room(b), b.plan, b.spending)).then(b.name.cmp(&a.name))
}

/// The same rank at the room placement reads live, before this run's charges.
fn live_rank(a: &Account, b: &Account) -> Ordering {
    Rank::of(a.room, a.plan, a.spending).order(&Rank::of(b.room, b.plan, b.spending)).then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str, eta_h: Option<f64>) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.takes(pools, account, eta_h)).max_by(|a, b| rank(pools, a, b))
}

/// A pin reason when placement, ranking the open accounts by the same rank at the room it reads
/// live, would pick another account than the one this run's earlier launches put first.
fn pin(capacity: &Capacity, pools: &Pools, host: &str, chosen: &Account) -> Option<String> {
    let placed = capacity.accounts.iter().filter(|account| account.provider() == chosen.provider() && account.refusal.is_none()).max_by(|a, b| live_rank(a, b))?;
    if placed.name == chosen.name {
        return None;
    }
    Some(if pools.pair(host, &placed.name) > 0 {
        format!("north dispatch ranks {} first on the room left after this run's earlier launches; placement ranks by the room it reads live and would pick {}", chosen.name, placed.name)
    } else {
        format!("north dispatch: placement order puts this box on {host}, where {} has a free worker slot and {} is not signed in", chosen.name, placed.name)
    })
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account, local: Option<String>) -> Launch {
    let pin = pin(capacity, pools, host, account);
    let burn = burn_line(pools, account, ready.eta_h);
    pools.take(host);
    pools.charge(&account.name, capacity.charge(account));
    Launch {
        item: ready.item.clone(),
        host: host.into(),
        account: account.name.clone(),
        brief: ready.brief.clone(),
        route: ready.route.clone(),
        cloud: false,
        local,
        pin,
        handoff: None,
        burn,
    }
}

/// Admits reviving a session bound to `host` and `account` by the slot and room rules a launch
/// meets there, taking the slot and its charge, with the pin placement needs when it would rank
/// another account first.
pub(crate) fn revive(capacity: &Capacity, pools: &mut Pools, host: &str, account: &str) -> Result<Option<String>, String> {
    let chosen = capacity.accounts.iter().find(|candidate| candidate.name == account).ok_or_else(|| format!("{account} has no pace sample"))?;
    chosen.open()?;
    if pools.host(host) == 0 {
        return Err(format!("{host} has no free slot"));
    }
    if pools.pair(host, account) == 0 {
        return Err(format!("{account} is not signed in on {host}"));
    }
    if !capacity.takes(pools, chosen, None) {
        return Err(format!("{account} has no room for another box this run"));
    }
    let pin = pin(capacity, pools, host, chosen).map(|_| format!("north dispatch revives the session on {account}, which holds its conversation"));
    pools.take(host);
    pools.charge(account, capacity.charge(chosen));
    Ok(pin)
}

/// Places one ready box, taking its slot from `pools`, or says why no slot takes it.
pub(crate) fn place(ready: &Ready, capacity: &Capacity, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let mut local = None;
    if ready.code_only {
        match capacity.cloud {
            Cloud::Reachable(credit) => {
                if pools.take_cloud() {
                    let account = format!("claude login on {}", capacity.here);
                    return Ok(Launch { item: ready.item.clone(), host: HOST.into(), account, brief: ready.brief.clone(), route: ready.route.clone(), cloud: true, local: None, pin: None, handoff: None, burn: None });
                }
                local = Some(format!("north dispatch: cloud lane open ({credit}) but its {SESSIONS} sessions are out"));
            }
            Cloud::Unreachable(why) => local = Some(format!("north dispatch: cloud lane unreachable from {}: {why}", capacity.here)),
            Cloud::Closed(_) => {}
        }
    }
    let mut refused = Vec::new();
    for candidate in candidates {
        if let Some(why) = &candidate.refusal {
            refused.push(format!("{} {why}", candidate.host));
            continue;
        }
        match best_account(capacity, pools, &candidate.host, ready.eta_h) {
            Some(account) => return Ok(launch(capacity, pools, ready, &candidate.host, account, local)),
            None if pools.host(&candidate.host) == 0 => refused.push(format!("{} has no free slot", candidate.host)),
            None => {
                let held = burn_held(capacity, pools, &candidate.host, ready.eta_h);
                refused.push(if held.is_empty() { format!("{} has no account with a free worker slot", candidate.host) } else { format!("{} holds the box: {}", candidate.host, held.join(", ")) });
            }
        }
    }
    Err(if refused.is_empty() { "no host to place it on".into() } else { refused.join("; ") })
}
