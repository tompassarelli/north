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
