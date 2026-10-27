//! Where each ready box goes: the cloud lane when it is reachable here and has a free session,
//! else the first host in placement order that serves the box and has a free slot.
use super::issues::IssueRef;
use super::cloud::{HOST, SESSIONS};
use super::slots::{Account, Pools};
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
    fn charge(&self, account: &Account) -> f64 {
        self.charge / account.plan
    }

    /// Whether the account can take one more box this run without crossing a window's stop.
    fn takes(&self, pools: &Pools, account: &Account) -> bool {
        pools.room(account).takes(self.charge(account))
    }
}

/// A live spend target first, since Tom set it to drain that account; then the week's room that
/// expires soonest, in plan units per hour until its reset; then five-hour room in plan units.
fn rank(pools: &Pools, a: &Account, b: &Account) -> Ordering {
    let (room_a, room_b) = (pools.room(a), pools.room(b));
    a.spending
        .cmp(&b.spending)
        .then(room_a.expiring(a.plan).total_cmp(&room_b.expiring(b.plan)))
        .then((room_a.five_hour * a.plan).total_cmp(&(room_b.five_hour * b.plan)))
        .then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.takes(pools, account)).max_by(|a, b| rank(pools, a, b))
}

/// A pin reason when an open account that ranks higher runs elsewhere, since placement on the
/// launching host would pick it over the one signed in where placement order puts the box.
fn pin(capacity: &Capacity, pools: &Pools, host: &str, chosen: &Account) -> Option<String> {
    let placed = capacity
        .accounts
        .iter()
        .filter(|account| account.provider() == chosen.provider() && account.refusal.is_none() && capacity.takes(pools, account))
        .max_by(|a, b| rank(pools, a, b))?;
    (placed.name != chosen.name && rank(pools, placed, chosen) == Ordering::Greater)
        .then(|| format!("north dispatch: placement order puts this box on {host}, where {} has a free worker slot and {} is not signed in", chosen.name, placed.name))
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account, local: Option<String>) -> Launch {
    let pin = pin(capacity, pools, host, account);
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
    }
}

/// Places one ready box, taking its slot from `pools`, or says why no slot takes it.
pub(crate) fn place(ready: &Ready, capacity: &Capacity, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let mut local = None;
    if ready.code_only {
        match capacity.cloud {
            Cloud::Reachable(credit) => {
                if pools.take_cloud() {
                    let account = format!("claude login on {}", capacity.here);
                    return Ok(Launch { item: ready.item.clone(), host: HOST.into(), account, brief: ready.brief.clone(), route: ready.route.clone(), cloud: true, local: None, pin: None, handoff: None });
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
        match best_account(capacity, pools, &candidate.host) {
            Some(account) => return Ok(launch(capacity, pools, ready, &candidate.host, account, local)),
            None if pools.host(&candidate.host) == 0 => refused.push(format!("{} has no free slot", candidate.host)),
            None => refused.push(format!("{} has no account with a free worker slot", candidate.host)),
        }
    }
    Err(if refused.is_empty() { "no host to place it on".into() } else { refused.join("; ") })
}
