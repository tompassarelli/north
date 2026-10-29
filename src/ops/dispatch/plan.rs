//! Where each ready box goes: the cloud lane when it is reachable here and has a free session,
//! else the first host in placement order that serves the box and has a free slot.
use super::issues::IssueRef;
use super::cloud::{HOST, SESSIONS};
use super::slots::{Account, Charge, Pools};
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

/// A live spend target first, since Tom set it to drain that account; then the week's room that
/// expires soonest, in plan units per hour until its reset; then five-hour room in plan units.
fn rank(pools: &Pools, a: &Account, b: &Account) -> Ordering {
    let (room_a, room_b) = (pools.room(a), pools.room(b));
    a.spending
        .cmp(&b.spending)
        .then(room_a.expiring(a.plan).total_cmp(&room_b.expiring(b.plan)))
        .then((room_a.five_hour * a.plan.five_hour).total_cmp(&(room_b.five_hour * b.plan.five_hour)))
        .then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str, eta_h: Option<f64>) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0 && capacity.takes(pools, account, eta_h)).max_by(|a, b| rank(pools, a, b))
}

/// A pin reason when an open account that ranks higher runs elsewhere, since placement on the
/// launching host would pick it over the one signed in where placement order puts the box; else
/// when another account has more headroom, since placement picks by the live gate's headroom where
/// dispatch ranks by spend target and expiring week room.
fn pin(capacity: &Capacity, pools: &Pools, host: &str, chosen: &Account, eta_h: Option<f64>) -> Option<String> {
    let peers = || capacity.accounts.iter().filter(|account| account.provider() == chosen.provider() && account.name != chosen.name);
    let placed = peers().filter(|account| account.refusal.is_none() && capacity.takes(pools, account, eta_h)).max_by(|a, b| rank(pools, a, b));
    if let Some(placed) = placed.filter(|placed| rank(pools, placed, chosen) == Ordering::Greater) {
        return Some(format!("north dispatch: placement order puts this box on {host}, where {} has a free worker slot and {} is not signed in", chosen.name, placed.name));
    }
    let roomier = peers().filter(|account| account.refusal.is_none() && account.room.headroom() > chosen.room.headroom()).max_by(|a, b| a.room.headroom().total_cmp(&b.room.headroom()))?;
    Some(format!(
        "north dispatch ranks {} first by spend target and week room expiring soonest; placement would pick {} for its {:.0}% headroom over {:.0}%",
        chosen.name,
        roomier.name,
        roomier.room.headroom(),
        chosen.room.headroom()
    ))
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account, local: Option<String>) -> Launch {
    let pin = pin(capacity, pools, host, account, ready.eta_h);
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
