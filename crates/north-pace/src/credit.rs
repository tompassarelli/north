//! Each API organization's credit grants, read from the operator's accounts file next to the orchestration
//! policy, and what the fleet's api sessions have spent of them at list price.
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use toml_edit::{DocumentMut, Item, Value};

use crate::foundation::clock::Timestamp;
use crate::router::pricing::{PriceTable, TokenUse};

#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub usd: f64,
    pub expires: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Balance {
    pub grant: Grant,
    pub spent: f64,
    pub left: f64,
    pub later: Vec<Grant>,
}

/// Each organization id's grants from `claude-accounts.toml` beside `policy`, soonest expiry first.
pub fn grants(policy: &Path) -> BTreeMap<String, Vec<Grant>> {
    let accounts: DocumentMut = std::fs::read_to_string(policy.with_file_name("claude-accounts.toml")).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let mut found: BTreeMap<String, Vec<Grant>> = BTreeMap::new();
    for org in accounts.get("account").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|account| account.get("api_org")) {
        let Some(id) = org.get("id").and_then(Item::as_str) else { continue };
        let listed = org.get("grants").and_then(Item::as_array).into_iter().flatten().filter_map(Value::as_inline_table);
        let grants = found.entry(id.to_owned()).or_default();
        grants.extend(listed.filter_map(|grant| Some(Grant { usd: grant.get("usd")?.as_float()?, expires: date(grant.get("expires")?)? })));
        grants.sort_by(|a, b| a.expires.cmp(&b.expires));
    }
    found
}

fn date(value: &Value) -> Option<String> {
    value.as_datetime().map(ToString::to_string).or_else(|| value.as_str().map(str::to_owned))
}

/// The list-price dollars of each api session's turns in `ledger`, as (UTC day, usd) per organization name from its `api:NAME` account.
pub fn spends(ledger: &Path, prices: &PriceTable) -> Result<BTreeMap<String, Vec<(String, f64)>>, String> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = db
        .prepare("SELECT account, model, started_ms, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write FROM runs WHERE account LIKE 'api:%' ORDER BY started_ms")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let tokens = TokenUse { input: row.get(3)?, output: row.get(4)?, cache_read: row.get(5)?, cache_write: row.get(6)? };
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?, tokens))
        })
        .map_err(|error| error.to_string())?;
    let mut found: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    for row in rows {
        let (account, model, started, tokens) = row.map_err(|error| error.to_string())?;
        let usd = model.as_deref().and_then(|model| prices.price(model)).map_or(0.0, |price| price.of(tokens));
        let org = account.strip_prefix("api:").unwrap_or(&account).to_owned();
        found.entry(org).or_default().push((Timestamp::from_unix_millis(started).iso_date(), usd));
    }
    Ok(found)
}

/// Charges each day's spend to the soonest-expiring grant still unexpired that day with credit left, the
/// overflow to the next, then reports the soonest grant unexpired on `today` that has credit left.
pub fn balance(grants: &[Grant], spends: &[(String, f64)], today: &str) -> Option<Balance> {
    let mut left: Vec<f64> = grants.iter().map(|grant| grant.usd).collect();
    for (day, usd) in spends {
        let open: Vec<usize> = (0..grants.len()).filter(|index| grants[*index].expires.as_str() >= day.as_str()).collect();
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
    let live: Vec<usize> = (0..grants.len()).filter(|index| grants[*index].expires.as_str() >= today).collect();
    let current = live.iter().copied().find(|index| left[*index] > 0.0).or_else(|| live.last().copied())?;
    Some(Balance { grant: grants[current].clone(), spent: grants[current].usd - left[current], left: left[current], later: live.iter().filter(|index| **index > current).map(|index| grants[*index].clone()).collect() })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub id: String,
    pub usd: f64,
    pub next_credit: Option<String>,
}

pub fn left(policy: &Path, spends: &BTreeMap<String, Vec<(String, f64)>>, today: &str) -> BTreeMap<String, Left> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let orgs = document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs")).and_then(Item::as_table_like);
    let named: Vec<(String, String)> = orgs.into_iter().flat_map(|orgs| orgs.iter()).filter_map(|(name, org)| Some((name.to_owned(), org.get("id")?.as_str()?.to_owned()))).collect();
    let grants = grants(policy);
    let next = next_credits(policy);
    named
        .iter()
        .map(|(name, id)| {
            let mine: Vec<(String, f64)> = named.iter().filter(|(_, other)| other == id).flat_map(|(other, _)| spends.get(other).cloned().unwrap_or_default()).collect();
            let usd = balance(grants.get(id).map_or(&[][..], Vec::as_slice), &mine, today).map_or(0.0, |balance| balance.left.max(0.0) + balance.later.iter().map(|grant| grant.usd).sum::<f64>());
            (name.clone(), Left { id: id.clone(), usd, next_credit: next.get(id).cloned() })
        })
        .collect()
}

fn next_credits(policy: &Path) -> BTreeMap<String, String> {
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

const BURN_WINDOW_S: i64 = 24 * 3600;

/// One pool entry as the router ranks it against plan accounts: what is left of its soonest unexpired grant,
/// when that grant expires, and the whole fleet's list-price spend per hour over the last day, the most it could send there.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub left: f64,
    pub expires_s: i64,
    pub burn_per_h: f64,
    pub exhausted: bool,
}

impl Source {
    /// Hours from `now_s` to the grant's expiry, which is treated as the start of its UTC day.
    pub fn deadline_h(&self, now_s: i64) -> f64 {
        (self.expires_s - now_s) as f64 / 3600.0
    }

    /// Whether the fleet, at its last day's burn, could spend what is left before the grant expires.
    pub fn drains(&self, now_s: i64) -> bool {
        self.burn_per_h * self.deadline_h(now_s) >= self.left
    }

    pub fn line(&self, now_s: i64) -> String {
        let pace = if self.drains(now_s) { "drains by then" } else { "cannot drain by then" };
        format!("${:.2} left, expires in {:.0}h; at the fleet's ${:.2}/h over the last day it {pace}", self.left, self.deadline_h(now_s), self.burn_per_h)
    }
}

/// The pool entries `[accounts.claude.api_orgs.NAME]` declares, as (name, organization id), the first name of each id.
pub fn orgs(policy: &Path) -> Vec<(String, String)> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let table = document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs")).and_then(Item::as_table_like);
    let mut named: Vec<(String, String)> = Vec::new();
    for (name, org) in table.into_iter().flat_map(|table| table.iter()) {
        let Some(id) = org.get("id").and_then(Item::as_str) else { continue };
        if !named.iter().any(|(_, seen)| seen == id) {
            named.push((name.to_owned(), id.to_owned()));
        }
    }
    named
}

/// Every pool entry with an unexpired grant, from the grants beside `policy`, the ledger and exhausted marks under `state` (the XDG state root).
pub fn sources(policy: &Path, state: &Path, now: Timestamp) -> Vec<Source> {
    let grants = grants(policy);
    let spends = PriceTable::load().map_err(|error| error.to_string()).and_then(|prices| spends(&state.join("north/ledger.db"), &prices)).unwrap_or_default();
    let (today, now_s) = (now.iso_date(), now.unix_seconds());
    let burn_per_h = PriceTable::load().ok().and_then(|prices| fleet_spend(&state.join("north/ledger.db"), &prices, now_s - BURN_WINDOW_S).ok()).unwrap_or(0.0) / (BURN_WINDOW_S as f64 / 3600.0);
    orgs(policy)
        .into_iter()
        .filter_map(|(name, id)| {
            let mine = spends.get(&name).cloned().unwrap_or_default();
            let balance = balance(grants.get(&id).map_or(&[][..], Vec::as_slice), &mine, &today)?;
            let expires_s = crate::foundation::clock::parse_day_number(&balance.grant.expires)? * 86_400;
            let exhausted = state.join("north/api-exhausted").join(&id).exists();
            Some(Source { name, left: balance.left, expires_s, burn_per_h, exhausted })
        })
        .collect()
}

/// The list-price dollars of every session's turns in `ledger` that started at or after `since_s`, on any account.
fn fleet_spend(ledger: &Path, prices: &PriceTable, since_s: i64) -> Result<f64, String> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = db
        .prepare("SELECT model, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write FROM runs WHERE started_ms >= ?1")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([since_s * 1000], |row| Ok((row.get::<_, Option<String>>(0)?, TokenUse { input: row.get(1)?, output: row.get(2)?, cache_read: row.get(3)?, cache_write: row.get(4)? })))
        .map_err(|error| error.to_string())?;
    let mut total = 0.0;
    for row in rows {
        let (model, tokens) = row.map_err(|error| error.to_string())?;
        total += model.as_deref().and_then(|model| prices.price(model)).map_or(0.0, |price| price.of(tokens));
    }
    Ok(total)
}
