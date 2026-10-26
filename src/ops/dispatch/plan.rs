//! Where each ready box goes: the cloud lane through a courier on this host when the lane is
//! reachable here and has a free courier slot, else the first host in placement order that serves the box and has a free slot.
use super::issues::IssueRef;
use super::slots::{Account, Pools};
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
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Launch {
    pub item: IssueRef,
    pub host: String,
    pub account: String,
    pub brief: String,
    /// Courier for the cloud lane rather than the worker itself.
    pub courier: bool,
    /// Why the box runs here instead of the cloud lane, recorded as `--local`.
    pub local: Option<String>,
    /// Why the account is not the one placement would pick, recorded as `--pin`.
    pub pin: Option<String>,
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
}

/// An account with a live spend target outranks headroom, since Tom set the target to drain it first.
fn rank(a: &Account, b: &Account) -> Ordering {
    a.spending.cmp(&b.spending).then(a.headroom.total_cmp(&b.headroom)).then(b.name.cmp(&a.name))
}

fn best_account<'a>(capacity: &'a Capacity, pools: &Pools, host: &str) -> Option<&'a Account> {
    capacity.accounts.iter().filter(|account| pools.pair(host, &account.name) > 0).max_by(|a, b| rank(a, b))
}

/// A pin reason when an open account with more headroom runs elsewhere, since placement on the
/// launching host would pick it over the one signed in where placement order puts the box.
fn pin(capacity: &Capacity, host: &str, chosen: &Account) -> Option<String> {
    let placed = capacity
        .accounts
        .iter()
        .filter(|account| account.provider() == chosen.provider() && account.refusal.is_none())
        .max_by(|a, b| rank(a, b))?;
    (placed.name != chosen.name && rank(placed, chosen) == Ordering::Greater)
        .then(|| format!("north dispatch: placement order puts this box on {host}, where {} has a free worker slot and {} is not signed in", chosen.name, placed.name))
}

fn launch(capacity: &Capacity, pools: &mut Pools, ready: &Ready, host: &str, account: &Account, courier: bool, local: Option<String>) -> Launch {
    pools.take(host);
    Launch {
        item: ready.item.clone(),
        host: host.into(),
        account: account.name.clone(),
        brief: ready.brief.clone(),
        courier,
        local,
        pin: pin(capacity, host, account),
    }
}

/// Places one ready box, taking its slot from `pools`, or says why no slot takes it.
pub(crate) fn place(ready: &Ready, capacity: &Capacity, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let mut local = None;
    if ready.code_only {
        match capacity.cloud {
            Cloud::Reachable(credit) => {
                let here = capacity.here;
                match best_account(capacity, pools, here) {
                    Some(account) => return Ok(launch(capacity, pools, ready, here, account, true, Some(format!("north dispatch: courier for the cloud lane ({credit})")))),
                    None => local = Some(format!("north dispatch: cloud lane open ({credit}) but {here} has no free slot for its courier")),
                }
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
            Some(account) => return Ok(launch(capacity, pools, ready, &candidate.host, account, false, local)),
            None if pools.host(&candidate.host) == 0 => refused.push(format!("{} has no free slot", candidate.host)),
            None => refused.push(format!("{} has no account with a free worker slot", candidate.host)),
        }
    }
    Err(if refused.is_empty() { "no host to place it on".into() } else { refused.join("; ") })
}
