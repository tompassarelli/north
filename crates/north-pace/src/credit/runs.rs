//! The api runs each host has recorded, its last day of runs on every account and the turns still open on it, as one host reports them
//! in a snapshot, and the merge of those snapshots with this host's ledger into the spend a grant is charged and the fleet's last day of spend.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::router::pricing::{PriceTable, TokenUse};

const SNAPSHOT_DAYS: i64 = 120;
const OPEN_FRESH_MS: i64 = 10 * 60 * 1000;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub session: String,
    pub account: Option<String>,
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

/// What one host knew of its spend at `read_ms`: the runs its ledger holds and the turns running on it then.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub host: String,
    pub read_ms: i64,
    pub finished: Vec<Finished>,
    pub open: Vec<Open>,
}

impl Snapshot {
    /// This host's snapshot: the api runs in `ledger` of the last 120 days, its runs on any account of the last day, and `open`, the turns running now.
    pub fn take(host: &str, ledger: &Path, open: Vec<Open>, now_ms: i64) -> Result<Self, String> {
        Ok(Self { host: host.to_owned(), read_ms: now_ms, finished: finished(ledger, now_ms - SNAPSHOT_DAYS * DAY_MS, now_ms - DAY_MS)?, open })
    }
}

fn finished(ledger: &Path, api_since_ms: i64, any_since_ms: i64) -> Result<Vec<Finished>, String> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = db
        .prepare("SELECT session, account, model, started_ms, ended_ms, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write FROM runs WHERE (account LIKE 'api:%' AND started_ms >= ?1) OR started_ms >= ?2 ORDER BY started_ms")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([api_since_ms, any_since_ms],|row| {
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

/// Every api spend as (UTC day, usd) per pool entry name, when each host that reported it was read,
/// and the list-price dollars of every run on any account and host that started in the day before the reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub orgs: BTreeMap<String, Vec<(String, f64)>>,
    pub hosts: Vec<(String, i64)>,
    pub last_day_usd: f64,
}

impl Reading {
    /// The footer under the usage table's api rows: the hosts whose spend was read (this host first), how old the oldest reading is at `now_ms`, and the `peers` that never reported.
    pub fn footer(&self, here: &str, peers: &[String], now_ms: i64) -> String {
        let reported = |host: &str| self.hosts.iter().any(|(reporter, _)| reporter == host);
        let read: Vec<&str> = std::iter::once(here).chain(self.hosts.iter().map(|(host, _)| host.as_str()).filter(|host| *host != here)).collect();
        let mut parts = vec![format!("API spend read on {}", listed(&read))];
        parts.extend(self.hosts.iter().map(|(_, read)| *read).min().map(|oldest| format!("oldest reading {}", age(now_ms - oldest))));
        let unread: Vec<&str> = peers.iter().map(String::as_str).filter(|peer| !reported(peer)).collect();
        if !unread.is_empty() {
            parts.push(format!("no reading from {}", listed(&unread)));
        }
        format!("{}.", parts.join("; "))
    }
}

fn listed(names: &[&str]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.join(""),
    }
}

fn age(millis: i64) -> String {
    match millis.max(0) / 1000 {
        0..60 => "under a minute".to_owned(),
        seconds @ 60..3600 => format!("{} min", seconds / 60),
        seconds => format!("{} h", seconds / 3600),
    }
}

/// The list-price dollars of each api run that started at or after `since_ms`, as (start in ms, usd), every host's report counted once.
pub fn api_runs(ledger: &Path, prices: &PriceTable, since_ms: i64) -> Result<Vec<(i64, f64)>, String> {
    let local = finished(ledger, since_ms, since_ms)?;
    let snapshots = stored(ledger);
    let mut seen: HashSet<(&str, i64)> = HashSet::new();
    let runs = local.iter().chain(snapshots.iter().flat_map(|snapshot| &snapshot.finished));
    Ok(runs.filter(|run| api_account(run).is_some() && run.started_ms >= since_ms && seen.insert((&run.session, run.started_ms))).map(|run| (run.started_ms, usd(prices, run.model.as_deref(), run.tokens))).collect())
}

/// The ledger's api runs and last day of runs, and every stored snapshot's.
pub fn read(ledger: &Path, prices: &PriceTable, now_ms: i64) -> Result<Reading, String> {
    Ok(merge(finished(ledger, 0, now_ms - DAY_MS)?, stored(ledger), prices, now_ms))
}

/// `local` and each snapshot's finished runs with a run counted once however many hosts report it, and each snapshot's open turns
/// counted while the snapshot is fresh and no run of the same session has ended since it was read; a run off the api counts only in the last day's spend.
pub fn merge(local: Vec<Finished>, snapshots: Vec<Snapshot>, prices: &PriceTable, now_ms: i64) -> Reading {
    let mut seen: HashSet<(&str, i64)> = HashSet::new();
    let mut latest_end: HashMap<&str, i64> = HashMap::new();
    let mut charges: Vec<(&str, i64, Option<&str>, TokenUse)> = Vec::new();
    let mut last_day_usd = 0.0;
    for run in local.iter().chain(snapshots.iter().flat_map(|snapshot| &snapshot.finished)) {
        if seen.insert((&run.session, run.started_ms)) {
            let end = latest_end.entry(&run.session).or_insert(run.ended_ms);
            *end = (*end).max(run.ended_ms);
            if run.started_ms >= now_ms - DAY_MS {
                last_day_usd += usd(prices, run.model.as_deref(), run.tokens);
            }
            if let Some(account) = api_account(run) {
                charges.push((account, run.started_ms, run.model.as_deref(), run.tokens));
            }
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
        orgs.entry(account.strip_prefix("api:").unwrap_or(account).to_owned()).or_default().push((Timestamp::from_unix_millis(at_ms).iso_date(), usd(prices, model, tokens)));
    }
    Reading { orgs, hosts: snapshots.iter().map(|snapshot| (snapshot.host.clone(), snapshot.read_ms)).collect(), last_day_usd }
}

fn api_account(run: &Finished) -> Option<&str> {
    run.account.as_deref().filter(|account| account.starts_with("api:"))
}

fn usd(prices: &PriceTable, model: Option<&str>, tokens: TokenUse) -> f64 {
    model.and_then(|model| prices.price(model)).map_or(0.0, |price| price.of(tokens))
}

#[cfg(test)]
mod tests;
