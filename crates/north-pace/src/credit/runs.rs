//! The api runs each host has recorded and the turns still open on it, as one host reports them in a snapshot,
//! and the merge of those snapshots with this host's ledger into the spend a grant is charged.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::router::pricing::{PriceTable, TokenUse};

const SNAPSHOT_DAYS: i64 = 120;
const OPEN_FRESH_MS: i64 = 10 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub session: String,
    pub account: String,
    pub model: Option<String>,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub tokens: TokenUse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Open {
    pub session: String,
    pub account: String,
    pub model: Option<String>,
    pub tokens: TokenUse,
}

/// What one host knew of its api spend at `read_ms`: the runs its ledger holds and the turns running on it then.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub host: String,
    pub read_ms: i64,
    pub finished: Vec<Finished>,
    pub open: Vec<Open>,
}

impl Snapshot {
    /// This host's snapshot: the api runs in `ledger` of the last 120 days, and `open`, the turns running now.
    pub fn take(host: &str, ledger: &Path, open: Vec<Open>, now_ms: i64) -> Result<Self, String> {
        let since = now_ms - SNAPSHOT_DAYS * 86_400_000;
        Ok(Self { host: host.to_owned(), read_ms: now_ms, finished: finished(ledger, since)?, open })
    }
}

fn finished(ledger: &Path, since_ms: i64) -> Result<Vec<Finished>, String> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = db
        .prepare("SELECT session, account, model, started_ms, ended_ms, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write FROM runs WHERE account LIKE 'api:%' AND started_ms >= ?1 ORDER BY started_ms")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([since_ms], |row| {
            let tokens = TokenUse { input: row.get(5)?, output: row.get(6)?, cache_read: row.get(7)?, cache_write: row.get(8)? };
            Ok(Finished { session: row.get(0)?, account: row.get(1)?, model: row.get(2)?, started_ms: row.get(3)?, ended_ms: row.get(4)?, tokens })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|error| error.to_string())
}

/// Where the snapshots pulled from other hosts are kept: beside `ledger`.
fn directory(ledger: &Path) -> PathBuf {
    ledger.with_file_name("api-spend")
}

fn file_name(host: &str) -> Option<String> {
    (!host.is_empty() && !host.starts_with('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).then(|| format!("{host}.json"))
}

/// Keeps `snapshot` as its host's latest reading beside `ledger`.
pub fn store(ledger: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let name = file_name(&snapshot.host).ok_or_else(|| format!("{:?} is not a host name", snapshot.host))?;
    let dir = directory(ledger);
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(format!(".{name}.tmp"));
    let text = serde_json::to_string(snapshot).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, dir.join(&name))).map_err(|error| format!("{}: {error}", dir.join(&name).display()))
}

fn stored(ledger: &Path) -> Vec<Snapshot> {
    let entries = std::fs::read_dir(directory(ledger)).into_iter().flatten().flatten();
    let mut found: Vec<Snapshot> = entries.filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).filter_map(|entry| serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()).collect();
    found.sort_by(|a, b| a.host.cmp(&b.host));
    found
}

/// Every api spend as (UTC day, usd) per pool entry name, and when each host that reported it was read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub orgs: BTreeMap<String, Vec<(String, f64)>>,
    pub hosts: Vec<(String, i64)>,
}

impl Reading {
    /// The hosts that reported and the oldest of their readings' age at `now_ms`, for the usage table.
    pub fn age_line(&self, now_ms: i64) -> String {
        let Some(oldest) = self.hosts.iter().map(|(_, read)| *read).min() else { return "read from this host only".to_owned() };
        let names: Vec<&str> = self.hosts.iter().map(|(host, _)| host.as_str()).collect();
        format!("read from this host and {}, the oldest {}", names.join(", "), age(now_ms - oldest))
    }
}

fn age(millis: i64) -> String {
    match millis.max(0) / 1000 {
        0..60 => "under a minute old".to_owned(),
        seconds @ 60..3600 => format!("{} min old", seconds / 60),
        seconds => format!("{} h old", seconds / 3600),
    }
}

/// The ledger's api runs and every stored snapshot's.
pub fn read(ledger: &Path, prices: &PriceTable, now_ms: i64) -> Result<Reading, String> {
    Ok(merge(finished(ledger, 0)?, stored(ledger), prices, now_ms))
}

/// `local` and each snapshot's finished runs with a run counted once however many hosts report it, and each snapshot's open turns
/// counted while the snapshot is fresh and no run of the same session has ended since it was read.
pub fn merge(local: Vec<Finished>, snapshots: Vec<Snapshot>, prices: &PriceTable, now_ms: i64) -> Reading {
    let mut seen: HashSet<(&str, i64)> = HashSet::new();
    let mut latest_end: HashMap<&str, i64> = HashMap::new();
    let mut charges: Vec<(&str, i64, Option<&str>, TokenUse)> = Vec::new();
    for run in local.iter().chain(snapshots.iter().flat_map(|snapshot| &snapshot.finished)) {
        if seen.insert((&run.session, run.started_ms)) {
            let end = latest_end.entry(&run.session).or_insert(run.ended_ms);
            *end = (*end).max(run.ended_ms);
            charges.push((&run.account, run.started_ms, run.model.as_deref(), run.tokens));
        }
    }
    for snapshot in snapshots.iter().filter(|snapshot| now_ms - snapshot.read_ms <= OPEN_FRESH_MS) {
        for turn in snapshot.open.iter().filter(|turn| latest_end.get(turn.session.as_str()).is_none_or(|end| *end <= snapshot.read_ms)) {
            charges.push((&turn.account, snapshot.read_ms, turn.model.as_deref(), turn.tokens));
        }
    }
    charges.sort_by_key(|charge| charge.1);
    let mut orgs: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    for (account, at_ms, model, tokens) in charges {
        let usd = model.and_then(|model| prices.price(model)).map_or(0.0, |price| price.of(tokens));
        orgs.entry(account.strip_prefix("api:").unwrap_or(account).to_owned()).or_default().push((Timestamp::from_unix_millis(at_ms).iso_date(), usd));
    }
    Reading { orgs, hosts: snapshots.iter().map(|snapshot| (snapshot.host.clone(), snapshot.read_ms)).collect() }
}

#[cfg(test)]
mod tests;
