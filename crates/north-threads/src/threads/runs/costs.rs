//! Usage readouts into the ledger, and each run's dollar cost: every rise of an account's weekly
//! window, priced by the operator's costs.toml, split across the runs that sent requests during it.
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};

use super::row::Row;
use crate::foundation::clock::Timestamp;
use crate::platform::usage_table;
use crate::threads::verb::{Failure, immediate};

const COLUMNS: [&str; 10] = ["ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source"];
const COST_WINDOWS: [&str; 3] = ["week", "weekly_all", "Gemini Models/weekly"];
const SAME_WINDOW_NANOS: i128 = 60_000_000_000;
const DAY_SECONDS: i64 = 86_400;
const WIDEST_LOOKBACK_DAYS: i64 = 64;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS run_cost_prices(account TEXT PRIMARY KEY, usd_per_week REAL NOT NULL) STRICT; \
                      CREATE INDEX IF NOT EXISTS run_usage_account ON run_usage(account, ts)";

/// Where each account's priced intervals may have changed: the earliest moment an ingest's new
/// readouts and usage samples fall at, keyed by account as run_costs keys it, and the runs it recorded.
#[derive(Default)]
pub struct Touched {
    since: HashMap<String, Timestamp>,
    runs: Vec<Sql>,
}

impl Touched {
    /// Refuses a timestamp not written in UTC as `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, since the re-price
    /// finds rows by comparing that text.
    pub fn add(&mut self, account: String, ts: &str) -> Result<(), Failure> {
        if !utc(ts) {
            return Err(Failure::Refused(format!("a usage timestamp is not written in UTC as YYYY-MM-DDTHH:MM:SSZ: {ts}")));
        }
        let at = moment(ts)?;
        self.since.entry(account).and_modify(|earliest| *earliest = (*earliest).min(at)).or_insert(at);
        Ok(())
    }

    pub fn run(&mut self, agent: Sql) {
        self.runs.push(agent);
    }
}

/// Appends each readout no earlier readout of the same window at the same time recorded, and marks
/// where each recorded cost-window readout falls.
pub fn record(db: &mut Connection, readouts: &[Row], touched: &mut Touched) -> Result<usize, Failure> {
    usage_table::ensure(db)?;
    let tx = db.transaction()?;
    let mut recorded = 0;
    {
        let same = COLUMNS[..5].iter().map(|column| format!("{column} IS ?")).collect::<Vec<_>>().join(" AND ");
        let mut insert = tx.prepare(&format!("INSERT INTO usage({}) SELECT {} WHERE NOT EXISTS (SELECT 1 FROM usage WHERE {same})", COLUMNS.join(", "), ["?"; 10].join(", ")))?;
        for readout in readouts {
            let values = COLUMNS.iter().chain(&COLUMNS[..5]).map(|column| readout.sql(column)).collect::<Result<Vec<_>, _>>()?;
            if insert.execute(params_from_iter(&values))? == 0 {
                continue;
            }
            recorded += 1;
            if let [Sql::Text(ts), provider, account, Sql::Text(kind), Sql::Text(name), used, ..] = &values[..]
                && kind == "window"
                && *used != Sql::Null
                && COST_WINDOWS.contains(&name.as_str())
            {
                touched.add(account_key(text(provider)?, text(account).ok())?, ts)?;
            }
        }
    }
    tx.commit()?;
    Ok(recorded)
}

fn text(value: &Sql) -> Result<String, Failure> {
    match value {
        Sql::Text(text) => Ok(text.clone()),
        _ => Err(Failure::Refused("a usage readout names its provider or account in something other than text".into())),
    }
}

fn account_key(provider: String, account: Option<String>) -> Result<String, Failure> {
    match provider.as_str() {
        "claude" => Ok(format!("claude-{}", account.ok_or_else(|| Failure::Refused("a claude usage readout has no account".into()))?)),
        _ => Ok(provider),
    }
}

struct Reading {
    ts: String,
    at: Timestamp,
    used: f64,
    resets: Option<String>,
}

struct Sample {
    at: Timestamp,
    agent: String,
    weight: f64,
}

type Key = (String, String, String, String);

/// Each interval's cost per run in the order the intervals were found; a repeated key keeps its
/// first place and its last cost.
#[derive(Default)]
struct Costs {
    order: Vec<(Key, f64)>,
    index: HashMap<Key, usize>,
}

impl Costs {
    fn insert(&mut self, key: Key, usd: f64) {
        match self.index.get(&key) {
            Some(&at) => self.order[at].1 = usd,
            None => {
                self.index.insert(key.clone(), self.order.len());
                self.order.push((key, usd));
            }
        }
    }
}

/// Re-prices the intervals of each touched account that end at or after its touch, or every interval
/// when the prices differ from the ones the stored intervals were priced at, then the cost_usd of each
/// run those intervals or the ingest touched, writing only rows whose value changed.
pub fn allocate(db: &mut Connection, prices: &BTreeMap<String, f64>, touched: &Touched) -> Result<(), Failure> {
    let tx = immediate(db)?;
    let total = "(SELECT SUM(cost_usd) FROM run_costs WHERE agent = runs.agent)";
    if repriced(&tx, prices)? {
        let fresh = intervals(&tx, prices, None)?;
        let stored: Vec<Key> = tx.prepare("SELECT account, started, ended, agent FROM run_costs")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?.collect::<Result<_, _>>()?;
        replace(&tx, &stored, &fresh)?;
        tx.execute(&format!("UPDATE runs SET cost_usd = {total} WHERE cost_usd IS NOT {total}"), [])?;
        return Ok(tx.commit()?);
    }
    let mut priced = BTreeSet::new();
    for (account, &since) in &touched.since {
        let fresh = intervals(&tx, prices, Some((account, since)))?;
        let stored = stored_since(&tx, account, since)?;
        replace(&tx, &stored, &fresh)?;
        priced.extend(stored.into_iter().chain(fresh.order.into_iter().map(|(key, _)| key)).map(|(_, _, _, agent)| agent));
    }
    let mut update = tx.prepare(&format!("UPDATE runs SET cost_usd = {total} WHERE agent = ? AND cost_usd IS NOT {total}"))?;
    for agent in touched.runs.iter().cloned().chain(priced.into_iter().map(Sql::Text)) {
        update.execute([agent])?;
    }
    drop(update);
    Ok(tx.commit()?)
}

/// Whether the prices differ from the ones run_costs was priced at, recording them when they do.
fn repriced(tx: &Transaction, prices: &BTreeMap<String, f64>) -> Result<bool, Failure> {
    let created = tx.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'run_cost_prices'", [], |_| Ok(())).optional()?.is_none();
    tx.execute_batch(SCHEMA)?;
    let priced: BTreeMap<String, f64> = tx.prepare("SELECT account, usd_per_week FROM run_cost_prices")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    if !created && priced == *prices {
        return Ok(false);
    }
    tx.execute("DELETE FROM run_cost_prices", [])?;
    for (account, usd) in prices {
        tx.execute("INSERT INTO run_cost_prices(account, usd_per_week) VALUES (?, ?)", params![account, usd])?;
    }
    Ok(true)
}

/// The account's stored intervals that end at or after `since`.
fn stored_since(tx: &Transaction, account: &str, since: Timestamp) -> Result<Vec<Key>, Failure> {
    let mut query = tx.prepare("SELECT started, ended, agent FROM run_costs WHERE account = ? AND ended >= ?")?;
    let rows = query.query_map(params![account, minute_bound(since)], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
    let mut stored = Vec::new();
    for row in rows {
        let (started, ended, agent) = row?;
        if moment(&ended)? >= since {
            stored.push((account.to_owned(), started, ended, agent));
        }
    }
    Ok(stored)
}

fn replace(tx: &Transaction, stored: &[Key], fresh: &Costs) -> Result<(), Failure> {
    for (account, started, ended, agent) in stored.iter().filter(|key| !fresh.index.contains_key(*key)) {
        tx.execute("DELETE FROM run_costs WHERE (account, started, ended, agent) = (?,?,?,?)", params![account, started, ended, agent])?;
    }
    for ((account, started, ended, agent), usd) in &fresh.order {
        tx.execute(
            "INSERT INTO run_costs(account, started, ended, agent, cost_usd) VALUES (?,?,?,?,?) ON CONFLICT(account, started, ended, agent) DO UPDATE SET cost_usd = excluded.cost_usd WHERE cost_usd IS NOT excluded.cost_usd",
            params![account, started, ended, agent, usd],
        )?;
    }
    Ok(())
}

fn moment(text: &str) -> Result<Timestamp, Failure> {
    Timestamp::parse_python_datetime(text).ok_or_else(|| Failure::Refused(format!("unreadable usage timestamp {text}")))
}

fn days_before(at: Timestamp, days: i64) -> Timestamp {
    Timestamp::from_unix_nanos(at.unix_nanos() - i128::from(days * DAY_SECONDS) * 1_000_000_000)
}

fn utc(ts: &str) -> bool {
    let bytes = ts.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes.get(range).is_some_and(|run| run.iter().all(u8::is_ascii_digit));
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')].iter().all(|&(at, byte)| bytes.get(at) == Some(&byte));
    separators && [0..4, 5..7, 8..10, 11..13, 14..16, 17..19].into_iter().all(digits) && ts.ends_with('Z')
}

/// A text bound every UTC ledger timestamp at or after `at` sorts above: the minute before it.
fn minute_bound(at: Timestamp) -> String {
    Timestamp::from_unix_nanos(at.unix_nanos() - 60_000_000_000).iso_minute()
}

fn same_window(a: Option<&str>, b: Option<&str>) -> Result<bool, Failure> {
    match (a, b) {
        _ if a == b => Ok(true),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Ok((moment(a)?.unix_nanos() - moment(b)?.unix_nanos()).abs() <= SAME_WINDOW_NANOS),
        _ => Ok(false),
    }
}

/// Whether the scan's plateau stays put from `earlier` to `later`: the same window at the same level.
fn level(earlier: &Reading, later: &Reading) -> Result<bool, Failure> {
    Ok(same_window(earlier.resets.as_deref(), later.resets.as_deref())? && later.used - earlier.used == 0.0)
}

/// The cost-window readouts in rowid order, with `only` one account's, from the given moment on when there is one.
fn readings_query(only: Option<(&str, Option<Timestamp>)>) -> (String, Vec<Sql>) {
    let mut sql = "SELECT provider, account, ts, used_pct, resets_at FROM usage WHERE kind = 'window' AND used_pct IS NOT NULL AND name IN (?, ?, ?)".to_owned();
    let mut bound: Vec<Sql> = COST_WINDOWS.iter().map(|name| Sql::Text((*name).to_owned())).collect();
    if let Some((account, since)) = only {
        let (provider, name) = account.strip_prefix("claude-").map_or((account, None), |name| ("claude", Some(name)));
        sql += " AND provider = ?";
        bound.push(Sql::Text(provider.to_owned()));
        if let Some(name) = name {
            sql += " AND account = ?";
            bound.push(Sql::Text(name.to_owned()));
        }
        if let Some(since) = since {
            sql += " AND ts >= ?";
            bound.push(Sql::Text(minute_bound(since)));
        }
    }
    sql += " ORDER BY rowid";
    (sql, bound)
}

/// Each account's readings sorted by time, the last reading at a timestamp replacing earlier ones in
/// place; with `only`, that one account's readings, from the given moment on when there is one.
fn windows(db: &Connection, only: Option<(&str, Option<Timestamp>)>) -> Result<Vec<(String, Vec<Reading>)>, Failure> {
    let (sql, bound) = readings_query(only);
    let since = only.and_then(|(_, since)| since);
    let mut query = db.prepare(&sql)?;
    let rows = query.query_map(params_from_iter(bound), |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?, row.get::<_, Option<String>>(4)?)))?;
    let mut accounts: Vec<(String, Vec<Reading>)> = Vec::new();
    let mut seen: HashMap<(usize, String), usize> = HashMap::new();
    for row in rows {
        let (provider, account, ts, used, resets) = row?;
        let account = account_key(provider, account)?;
        let index = accounts.iter().position(|(name, _)| *name == account).unwrap_or_else(|| {
            accounts.push((account, Vec::new()));
            accounts.len() - 1
        });
        let reading = Reading { at: moment(&ts)?, ts, used, resets };
        let readings = &mut accounts[index].1;
        match seen.entry((index, reading.ts.clone())) {
            Entry::Occupied(at) => readings[*at.get()] = reading,
            Entry::Vacant(slot) => {
                slot.insert(readings.len());
                readings.push(reading);
            }
        }
    }
    for (_, readings) in &mut accounts {
        readings.sort_by_key(|reading| reading.at);
        if let Some(since) = since {
            readings.retain(|reading| reading.at >= since);
        }
    }
    Ok(accounts)
}

/// Each account's usage samples sorted by time; with `only`, that one account's samples from the given moment on.
fn samples(db: &Connection, only: Option<(&str, Timestamp)>) -> Result<HashMap<String, Vec<Sample>>, Failure> {
    let mut by_account: HashMap<String, Vec<Sample>> = HashMap::new();
    let filter = if only.is_some() { " WHERE account = ?1 AND ts >= ?2" } else { "" };
    let bound: Vec<Sql> = only.map_or_else(Vec::new, |(account, since)| vec![Sql::Text(account.to_owned()), Sql::Text(minute_bound(since))]);
    let mut query = db.prepare(&format!("SELECT account, ts, agent, weight FROM run_usage{filter} ORDER BY rowid"))?;
    for row in query.query_map(params_from_iter(bound), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?)))? {
        let (account, ts, agent, weight) = row?;
        let at = moment(&ts)?;
        if only.is_none_or(|(_, since)| at >= since) {
            by_account.entry(account).or_default().push(Sample { at, agent, weight });
        }
    }
    for samples in by_account.values_mut() {
        samples.sort_by_key(|sample| sample.at);
    }
    Ok(by_account)
}

/// The priced intervals of every account, or with `only` those of one account that end at or after
/// the given moment.
fn intervals(db: &Connection, prices: &BTreeMap<String, f64>, only: Option<(&str, Timestamp)>) -> Result<Costs, Failure> {
    let mut fresh = Costs::default();
    let Some((account, since)) = only else {
        let by_account = samples(db, None)?;
        for (account, readings) in windows(db, None)? {
            if let Some(&price) = prices.get(&account) {
                rises(&mut fresh, &account, &readings, by_account.get(&account).map_or(&[][..], Vec::as_slice), price)?;
            }
        }
        return Ok(fresh);
    };
    let Some(&price) = prices.get(account) else { return Ok(fresh) };
    let mut days = 1;
    let (readings, start) = loop {
        let whole = days > WIDEST_LOOKBACK_DAYS;
        let from = Some((account, (!whole).then(|| days_before(since, days))));
        let readings = windows(db, from)?.into_iter().find(|(name, _)| name == account).map(|(_, readings)| readings).unwrap_or_default();
        if let Some(start) = plateau(&readings, since, whole)? {
            break (readings, start);
        }
        days *= 4;
    };
    let Some(initial) = readings.get(start) else { return Ok(fresh) };
    let by_account = samples(db, Some((account, initial.at)))?;
    rises(&mut fresh, account, &readings[start..], by_account.get(account).map_or(&[][..], Vec::as_slice), price)?;
    Ok(fresh)
}

/// Where the scan's plateau stands when it reaches the first reading at or after `since`: the first of
/// the unbroken run of level readings just before it. None when that run may reach past the first
/// loaded reading and `whole` says the readings are not the account's whole history.
fn plateau(readings: &[Reading], since: Timestamp, whole: bool) -> Result<Option<usize>, Failure> {
    let mut start = readings.partition_point(|reading| reading.at < since).saturating_sub(1);
    while start > 0 && level(&readings[start - 1], &readings[start])? {
        start -= 1;
    }
    Ok((start > 0 || whole).then_some(start))
}

/// Each rise within one window, from the plateau it left to the reading that ends it, priced and
/// split by the weight each run sent after the plateau and by the reading.
fn rises(fresh: &mut Costs, account: &str, readings: &[Reading], events: &[Sample], price: f64) -> Result<(), Failure> {
    let mut plateau = 0;
    for after in 1..readings.len() {
        let (earlier, later) = (&readings[after - 1], &readings[after]);
        if !same_window(earlier.resets.as_deref(), later.resets.as_deref())? || later.used < earlier.used {
            plateau = after;
            continue;
        }
        let change = later.used - earlier.used;
        if change == 0.0 {
            continue;
        }
        let initial = &readings[plateau];
        let first = events.partition_point(|sample| sample.at <= initial.at);
        let last = events.partition_point(|sample| sample.at <= later.at).max(first);
        split(fresh, (account, &initial.ts, &later.ts), change * price / 100.0, &events[first..last]);
        plateau = after;
    }
    Ok(())
}

fn split(fresh: &mut Costs, (account, started, ended): (&str, &str, &str), cost: f64, events: &[Sample]) {
    let mut weights: Vec<(&str, f64)> = Vec::new();
    let mut place: HashMap<&str, usize> = HashMap::new();
    for sample in events {
        match place.entry(&sample.agent) {
            Entry::Occupied(at) => weights[*at.get()].1 += sample.weight,
            Entry::Vacant(slot) => {
                slot.insert(weights.len());
                weights.push((&sample.agent, 0.0 + sample.weight));
            }
        }
    }
    let total = weights.iter().fold(0.0, |sum, (_, weight)| sum + weight);
    if total == 0.0 {
        return;
    }
    for (agent, weight) in weights {
        fresh.insert((account.to_owned(), started.to_owned(), ended.to_owned(), agent.to_owned()), cost * weight / total);
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params_from_iter};

    use super::readings_query;
    use crate::foundation::clock::Timestamp;
    use crate::platform::usage_table;

    #[test]
    fn one_claude_account_s_recent_readings_are_searched_by_account_and_time_not_scanned_by_provider() {
        let db = Connection::open_in_memory().unwrap();
        usage_table::ensure(&db).unwrap();
        let (sql, bound) = readings_query(Some(("claude-a", Some(Timestamp::parse("2026-11-06T10:00:00Z").unwrap()))));
        let plan: Vec<String> = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap().query_map(params_from_iter(bound), |row| row.get(3)).unwrap().collect::<Result<_, _>>().unwrap();
        assert!(plan.iter().any(|step| step.contains("usage_latest (provider=? AND account=? AND kind=? AND name=? AND ts>?)")), "{plan:?}");
    }

    #[test]
    fn a_touch_reads_from_the_minute_before_it_and_refuses_a_timestamp_with_an_offset() {
        let bound = super::minute_bound(Timestamp::parse("2026-11-06T10:00:30Z").unwrap());
        for kept in ["2026-11-06T10:00:30Z", "2026-11-06T10:00:30.250Z", "2026-11-06T10:01:00Z", "2026-11-07T00:00:00Z"] {
            assert!(kept > bound.as_str(), "{kept} sorts below {bound}");
        }
        assert!("2026-11-06T09:30:00Z" < bound.as_str(), "{bound} reads the half hour before the touch");
        let mut touched = super::Touched::default();
        assert!(touched.add("claude-a".to_owned(), "2026-11-06T10:00:30.250Z").is_ok());
        assert!(touched.add("claude-a".to_owned(), "2026-11-06T12:00:30+02:00").is_err());
    }
}
