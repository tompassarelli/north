//! Each API organization's credit grants, read from the operator's accounts file next to the orchestration
//! policy, and what the fleet's api sessions have spent of them at list price.
use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Value};

use crate::foundation::clock::Timestamp;
use crate::router::pricing::PriceTable;

pub mod marks;
pub mod readings;
mod runs;
pub use runs::{Finished, Open, Reading, Snapshot, api_runs, store};

#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub usd: f64,
    /// The UTC day the credit was granted, empty when the file does not say.
    pub granted: String,
    pub expires: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Balance {
    pub grant: Grant,
    pub spent: f64,
    pub left: f64,
    pub later: Vec<Grant>,
}

impl Balance {
    pub fn exhausted(&self) -> bool {
        !has_credit(self.left)
    }
}

/// The other machines `[hosts.NAME]` declares an `ssh` or `deploy_ssh` destination for, whose api spend this host pulls.
pub fn peers(policy: &Path) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let hosts = document.get("hosts").and_then(Item::as_table_like);
    let here = crate::platform::host::name();
    let declared = hosts.into_iter().flat_map(|hosts| hosts.iter());
    declared.filter(|(name, host)| *name != here && ["ssh", "deploy_ssh"].iter().any(|key| host.get(key).is_some_and(|destination| destination.as_str().is_some()))).map(|(name, _)| name.to_owned()).collect()
}

/// Each organization id's grants from `claude-accounts.toml` beside `policy`, soonest expiry first.
pub fn grants(policy: &Path) -> BTreeMap<String, Vec<Grant>> {
    let accounts: DocumentMut = std::fs::read_to_string(policy.with_file_name("claude-accounts.toml")).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let mut found: BTreeMap<String, Vec<Grant>> = BTreeMap::new();
    for org in accounts.get("account").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|account| account.get("api_org")) {
        let Some(id) = org.get("id").and_then(Item::as_str) else { continue };
        let listed = org.get("grants").and_then(Item::as_array).into_iter().flatten().filter_map(Value::as_inline_table);
        let grants = found.entry(id.to_owned()).or_default();
        grants.extend(listed.filter_map(|grant| Some(Grant { usd: grant.get("usd")?.as_float()?, granted: grant.get("granted").and_then(date).unwrap_or_default(), expires: date(grant.get("expires")?)? })));
        grants.sort_by(|a, b| a.expires.cmp(&b.expires));
    }
    found
}

fn date(value: &Value) -> Option<String> {
    value.as_datetime().map(ToString::to_string).or_else(|| value.as_str().map(str::to_owned))
}

/// The list-price dollars of each api session's turns, as (UTC day, usd) per organization name from its `api:NAME` account: `ledger`'s runs, the other hosts' snapshots beside it and the turns still open.
pub fn spends(ledger: &Path, prices: &PriceTable, now: Timestamp) -> Result<Reading, String> {
    runs::read(ledger, prices, now.unix_millis())
}

const CENT: f64 = 0.005;

fn has_credit(left: f64) -> bool {
    left >= CENT
}

/// What each grant has left after charging each day's spend to the soonest-expiring grant still unexpired that day with credit left, the overflow to the next.
fn remaining(grants: &[Grant], spends: &[(String, f64)]) -> Vec<f64> {
    let mut left: Vec<f64> = grants.iter().map(|grant| grant.usd).collect();
    for (day, usd) in spends {
        let open: Vec<usize> = (0..grants.len()).filter(|index| grants[*index].granted.as_str() <= day.as_str() && grants[*index].expires.as_str() >= day.as_str()).collect();
        let mut rest = *usd;
        for index in &open {
            let taken = rest.min(left[*index].max(0.0));
            left[*index] -= taken;
            rest -= taken;
        }
        if let Some(last) = open.last() {
            left[*last] -= rest;
        }
    }
    left
}

/// Each grant's credit left after charging `spends`' (UTC day, usd) to the soonest-expiring grant granted
/// on or before that day and unexpired then, the overflow to the next; a day no grant covers is dropped.
pub fn left_after(grants: &[Grant], spends: &[(String, f64)]) -> Vec<f64> {
    remaining(grants, spends)
}

/// The grants unexpired on `today` a launch may draw on, from the soonest one list-price `spends` leave credit on, else the last of them: counted spend only ranks an
/// organization, since only the provider's refusal of a turn for credit closes it; soonest expiry first.
pub fn launchable(grants: &[Grant], spends: &[(String, f64)], today: &str) -> Vec<Grant> {
    let left = remaining(grants, spends);
    let live: Vec<usize> = (0..grants.len()).filter(|index| grants[*index].expires.as_str() >= today).collect();
    let current = live.iter().position(|index| has_credit(left[*index])).unwrap_or(live.len().saturating_sub(1));
    live[current..].iter().map(|index| grants[*index].clone()).collect()
}

/// Reports the soonest grant unexpired on `today` that has credit left, with what is spent of it and the grants after it.
pub fn balance(grants: &[Grant], spends: &[(String, f64)], today: &str) -> Option<Balance> {
    let left = remaining(grants, spends);
    let live: Vec<usize> = (0..grants.len()).filter(|index| grants[*index].expires.as_str() >= today).collect();
    let current = live.iter().copied().find(|index| has_credit(left[*index])).or_else(|| live.last().copied())?;
    Some(Balance { grant: grants[current].clone(), spent: grants[current].usd - left[current], left: left[current], later: live.iter().filter(|index| **index > current).map(|index| grants[*index].clone()).collect() })
}

/// Each organization id's grants a launch may draw on `today`, read from the ledger's api spend; an id appears only when it has dated grants, so an empty list means every grant expired.
pub fn open_grants(policy: &Path, ledger: &Path, today: &str) -> BTreeMap<String, Vec<Grant>> {
    let spends = PriceTable::load().map_err(|error| error.to_string()).and_then(|prices| spends(ledger, &prices, Timestamp::now())).map(|reading| reading.orgs).unwrap_or_default();
    let named = named(policy);
    grants(policy)
        .into_iter()
        .filter(|(_, grants)| !grants.is_empty())
        .map(|(id, grants)| {
            let mine: Vec<(String, f64)> = named.iter().filter(|(_, other)| *other == id).flat_map(|(name, _)| spends.get(name).cloned().unwrap_or_default()).collect();
            (id, launchable(&grants, &mine, today))
        })
        .collect()
}

/// Every pool entry name `[accounts.claude.api_orgs.NAME]` declares, as (name, organization id).
fn named(policy: &Path) -> Vec<(String, String)> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let orgs = document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs")).and_then(Item::as_table_like);
    orgs.into_iter().flat_map(|orgs| orgs.iter()).filter_map(|(name, org)| Some((name.to_owned(), org.get("id")?.as_str()?.to_owned()))).collect()
}

/// One api organization's credit as every operator view and placement read it (north#1063): the only place north works out what an organization has left.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    /// The first pool entry name that declares the organization, and every name that does.
    pub name: String,
    pub names: Vec<String>,
    pub id: String,
    /// The soonest unexpired grant with credit left at list price, what is spent of it and the grants after it; none when no grant is unexpired.
    pub balance: Option<Balance>,
    /// North's list-price estimate of the credit left on every unexpired grant.
    pub estimated: f64,
    pub reading: Option<readings::Reading>,
    /// The credit left on every unexpired grant: the console or provider reading where one is recorded, else the estimate.
    pub left: f64,
    /// Whether the provider refused a turn for low credit and the mark still holds; only that closes the organization.
    pub exhausted: bool,
    pub next_credit: Option<String>,
}

impl Standing {
    /// The part of `left` on the soonest-expiring grant: the later grants are charged only after it, so they hold their face value.
    pub fn expiring(&self) -> f64 {
        let later: f64 = self.balance.iter().flat_map(|balance| &balance.later).map(|grant| grant.usd).sum();
        (self.left - later).max(0.0)
    }

    /// What is left and where the figure comes from, worded the same in every view: `$X left (console reading TIME, estimate $Y)`.
    pub fn figure(&self) -> String {
        if self.exhausted {
            return "exhausted".to_owned();
        }
        let read = self.reading.as_ref().map_or_else(String::new, |reading| format!(" ({} reading {}, estimate ${:.2})", reading.source.label(), Timestamp::from_unix_seconds(reading.at).iso_minute(), self.estimated));
        format!("${:.2} left{read}", self.left)
    }

    /// The organization's account and its figure: the line `north dash` and the fleet screen print.
    pub fn line(&self) -> String {
        format!("api:{} {}", self.name, self.figure())
    }
}

/// Every api organization's credit and the list-price spend it was charged with, read once from the grants beside `policy`, the ledger and the other hosts' snapshots, readings and exhausted marks under `state` (the XDG state root).
pub struct Credits {
    pub orgs: Vec<Standing>,
    pub spend: Result<Reading, String>,
}

impl Credits {
    /// The whole fleet's list-price api spend per hour over the last day.
    pub fn burn_per_h(&self) -> f64 {
        self.spend.as_ref().map_or(0.0, |reading| reading.last_day_usd / 24.0)
    }

    /// The organization a pool entry name declares.
    pub fn named(&self, name: &str) -> Option<&Standing> {
        self.orgs.iter().find(|org| org.names.iter().any(|named| named == name))
    }
}

pub fn standing(policy: &Path, state: &Path, now: Timestamp) -> Credits {
    let spend = PriceTable::load().map_err(|error| error.to_string()).and_then(|prices| spends(&state.join("north/ledger.db"), &prices, now));
    let empty = BTreeMap::new();
    let spends = spend.as_ref().map_or(&empty, |reading| &reading.orgs);
    let (named, grants, next, today) = (named(policy), grants(policy), next_credits(policy), now.iso_date());
    let readings = readings::directory(state);
    let orgs = orgs(policy)
        .into_iter()
        .map(|(name, id)| {
            let names: Vec<String> = named.iter().filter(|(_, other)| *other == id).map(|(other, _)| other.clone()).collect();
            let mine: Vec<(String, f64)> = names.iter().flat_map(|other| spends.get(other).cloned().unwrap_or_default()).collect();
            let balance = balance(grants.get(&id).map_or(&[][..], Vec::as_slice), &mine, &today);
            let estimated = balance.as_ref().map_or(0.0, |balance| balance.left.max(0.0) + balance.later.iter().map(|grant| grant.usd).sum::<f64>());
            let reading = readings::get(&readings, &id);
            let left = reading.as_ref().map_or(estimated, |reading| reading.usd);
            let exhausted = marks::held(&state.join("north/api-exhausted"), &id, &today);
            Standing { name, names, next_credit: next.get(&id).cloned(), id, balance, estimated, reading, left, exhausted }
        })
        .collect();
    Credits { orgs, spend }
}

/// The credit each grant still held on the day it expired, as (expiry, usd), for the grants that expired from `since_day` up to `today`, charged by every pool entry's list-price `spends` of its organization.
pub fn expired_unspent(policy: &Path, spends: &BTreeMap<String, Vec<(String, f64)>>, since_day: &str, today: &str) -> Vec<(String, f64)> {
    let named = named(policy);
    let mut unspent = Vec::new();
    for (id, grants) in grants(policy) {
        let mine: Vec<(String, f64)> = named.iter().filter(|(_, other)| *other == id).flat_map(|(name, _)| spends.get(name).cloned().unwrap_or_default()).collect();
        let left = remaining(&grants, &mine);
        unspent.extend(grants.iter().zip(left).filter(|(grant, left)| grant.expires.as_str() < today && grant.expires.as_str() >= since_day && has_credit(*left)).map(|(grant, left)| (grant.expires.clone(), left)));
    }
    unspent
}

/// Each organization id's next credit day from `claude-accounts.toml` beside `policy`, where it declares one.
pub fn next_credits(policy: &Path) -> BTreeMap<String, String> {
    let accounts: DocumentMut = std::fs::read_to_string(policy.with_file_name("claude-accounts.toml")).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    accounts
        .get("account")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten()
        .filter_map(|account| account.get("api_org"))
        .filter_map(|org| Some((org.get("id")?.as_str()?.to_owned(), date(org.get("next_credit")?.as_value()?)?)))
        .collect()
}

/// One pool entry as the router ranks it against plan accounts: its credit, when its soonest unexpired grant expires,
/// and the whole fleet's list-price spend per hour over the last day, the most it could send there.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub credit: Standing,
    pub expires_s: i64,
    pub burn_per_h: f64,
}

impl Source {
    /// Hours from `now_s` to the grant's expiry, which is treated as the start of its UTC day.
    pub fn deadline_h(&self, now_s: i64) -> f64 {
        (self.expires_s - now_s) as f64 / 3600.0
    }

    /// Whether the fleet, at its last day's burn, could spend what is left of the soonest grant before it expires.
    pub fn drains(&self, now_s: i64) -> bool {
        self.burn_per_h * self.deadline_h(now_s) >= self.credit.expiring()
    }

    pub fn line(&self, now_s: i64) -> String {
        let pace = if self.drains(now_s) { "drains by then" } else { "cannot drain by then" };
        format!("{}, expires in {:.0}h; at the fleet's ${:.2}/h over the last day it {pace}", self.credit.figure(), self.deadline_h(now_s), self.burn_per_h)
    }
}

/// The pool entries `[accounts.claude.api_orgs.NAME]` declares, as (name, organization id), the first name of each id.
pub fn orgs(policy: &Path) -> Vec<(String, String)> {
    let mut first: Vec<(String, String)> = Vec::new();
    for (name, id) in named(policy) {
        if !first.iter().any(|(_, seen)| *seen == id) {
            first.push((name, id));
        }
    }
    first
}

/// Every organization in `credits` with an unexpired grant, as placement ranks it.
pub fn sources(credits: &Credits) -> Vec<Source> {
    let burn_per_h = credits.burn_per_h();
    credits
        .orgs
        .iter()
        .filter_map(|credit| {
            let expires_s = crate::foundation::clock::parse_day_number(&credit.balance.as_ref()?.grant.expires)? * 86_400;
            Some(Source { credit: credit.clone(), expires_s, burn_per_h })
        })
        .collect()
}
