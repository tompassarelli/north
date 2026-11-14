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
pub const FREE_MODEL: &str = "opencode/deepseek-v4.1-flash-free";
const SAME_WINDOW_NANOS: i128 = 60_000_000_000;
const DAY_SECONDS: i64 = 86_400;
const WIDEST_LOOKBACK_DAYS: i64 = 64;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS run_cost_prices(account TEXT PRIMARY KEY, usd_per_week REAL NOT NULL) STRICT; \
                      CREATE INDEX IF NOT EXISTS run_usage_account ON run_usage(account, ts); \
                      CREATE INDEX IF NOT EXISTS usage_window_time ON usage(provider, kind, name, ts)";

/// Where each account's priced intervals may have changed: the earliest and latest moments an
/// ingest's new readouts and usage samples fall at, keyed by account as run_costs keys it, and the
/// runs it recorded.
#[derive(Default)]
pub struct Touched {
    spans: HashMap<String, Span>,
    runs: Vec<Sql>,
}

#[derive(Clone, Copy)]
struct Span {
    since: Timestamp,
    until: Timestamp,
}

impl Touched {
    /// Refuses a timestamp not written in UTC as `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, since the re-price
    /// finds rows by comparing that text.
    pub fn add(&mut self, account: String, ts: &str) -> Result<(), Failure> {
        if !utc(ts) {
            return Err(Failure::Refused(format!("a usage timestamp is not written in UTC as YYYY-MM-DDTHH:MM:SSZ: {ts}")));
        }
        let at = moment(ts)?;
        let span = self.spans.entry(account).or_insert(Span { since: at, until: at });
        span.since = span.since.min(at);
        span.until = span.until.max(at);
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

/// Re-prices the intervals of each touched account that end from its earliest touch through the first
/// rise or reset after its latest one, or every interval when the prices differ from the ones the
/// stored intervals were priced at, then the cost_usd of each
/// run those intervals or the ingest touched, writing only rows whose value changed. A run whose session reported
/// its cost keeps that cost, and a run on the free model has no interval and costs its price, 0.
pub fn allocate(db: &mut Connection, prices: &BTreeMap<String, f64>, touched: &Touched) -> Result<(), Failure> {
    let tx = immediate(db)?;
    let total = &format!("coalesce(session_usd, (SELECT SUM(cost_usd) FROM run_costs WHERE agent = runs.agent), CASE WHEN model = '{FREE_MODEL}' THEN 0 END)");
    if repriced(&tx, prices)? {
        let fresh = every_interval(&tx, prices)?;
        let stored: Vec<Key> = tx.prepare("SELECT account, started, ended, agent FROM run_costs")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?.collect::<Result<_, _>>()?;
        replace(&tx, &stored, &fresh)?;
        tx.execute(&format!("UPDATE runs SET cost_usd = {total} WHERE cost_usd IS NOT {total}"), [])?;
        return Ok(tx.commit()?);
    }
    let mut priced = BTreeSet::new();
    for (account, &span) in &touched.spans {
        let (fresh, through) = match prices.get(account) {
            Some(&price) => account_intervals(&tx, account, span, price)?,
            None => (Costs::default(), None),
        };
        let stored = stored_within(&tx, account, span.since, through)?;
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

/// The account's stored intervals that end at or after `since`, and through `through` when there is one.
fn stored_within(tx: &Transaction, account: &str, since: Timestamp, through: Option<Timestamp>) -> Result<Vec<Key>, Failure> {
    let mut bound = vec![Sql::Text(account.to_owned()), Sql::Text(minute_bound(since))];
    bound.extend(through.map(|through| Sql::Text(minute_after(through))));
    let mut query = tx.prepare(&format!("SELECT started, ended, agent FROM run_costs WHERE account = ? AND ended >= ?{}", if through.is_some() { " AND ended < ?" } else { "" }))?;
    let rows = query.query_map(params_from_iter(bound), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
    let mut stored = Vec::new();
    for row in rows {
        let (started, ended, agent) = row?;
        let at = moment(&ended)?;
        if at >= since && through.is_none_or(|through| at <= through) {
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

/// A text bound every UTC ledger timestamp at or before `at` sorts below, and every timestamp from
/// the second minute after it sorts above.
fn minute_after(at: Timestamp) -> String {
    Timestamp::from_unix_nanos(at.unix_nanos() + 60_000_000_000).iso_minute()
}

fn same_window(a: Option<&str>, b: Option<&str>) -> Result<bool, Failure> {
    match (a, b) {
        _ if a == b => Ok(true),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Ok((moment(a)?.unix_nanos() - moment(b)?.unix_nanos()).abs() <= SAME_WINDOW_NANOS),
        _ => Ok(false),
    }
}

/// What the scan does from one reading to the next: a new window or a drop starts a new plateau, the
/// same level keeps it, and a rise ends an interval.
enum Step {
    Reset,
    Level,
    Rise(f64),
}

fn step(earlier: &Reading, later: &Reading) -> Result<Step, Failure> {
    if !same_window(earlier.resets.as_deref(), later.resets.as_deref())? || later.used < earlier.used {
        return Ok(Step::Reset);
    }
    let change = later.used - earlier.used;
    Ok(if change == 0.0 { Step::Level } else { Step::Rise(change) })
}

/// The cost-window readouts, with `account` only that account's.
fn readings_filter(account: Option<&str>) -> (String, Vec<Sql>) {
    let mut sql = "FROM usage WHERE kind = 'window' AND used_pct IS NOT NULL AND name IN (?, ?, ?)".to_owned();
    let mut bound: Vec<Sql> = COST_WINDOWS.iter().map(|name| Sql::Text((*name).to_owned())).collect();
    if let Some(account) = account {
        let (provider, name) = account.strip_prefix("claude-").map_or((account, None), |name| ("claude", Some(name)));
        sql += " AND provider = ?";
        bound.push(Sql::Text(provider.to_owned()));
        if let Some(name) = name {
            sql += " AND account = ?";
            bound.push(Sql::Text(name.to_owned()));
        }
    }
    (sql, bound)
}

/// The moments one account's readings are loaded between: from the minute before `from` and through
/// the minute after `through`, each unbounded when absent.
#[derive(Clone, Copy, Default)]
struct Range {
    from: Option<Timestamp>,
    through: Option<Timestamp>,
}

/// The cost-window readouts in rowid order, with `only` one account's within its range.
fn readings_query(only: Option<(&str, Range)>) -> (String, Vec<Sql>) {
    let (filter, mut bound) = readings_filter(only.map(|(account, _)| account));
    let mut sql = format!("SELECT provider, account, ts, used_pct, resets_at {filter}");
    let range = only.map(|(_, range)| range).unwrap_or_default();
    if let Some(from) = range.from {
        sql += " AND ts >= ?";
        bound.push(Sql::Text(minute_bound(from)));
    }
    if let Some(through) = range.through {
        sql += " AND ts < ?";
        bound.push(Sql::Text(minute_after(through)));
    }
    sql += " ORDER BY rowid";
    (sql, bound)
}

/// A reading of the account from more than a second before `since`, the latest such, with its level:
/// every reading from it on holds the last reading before `since`.
fn latest_before(db: &Connection, account: &str, since: Timestamp) -> Result<Option<(Timestamp, f64)>, Failure> {
    let (filter, bound) = readings_filter(Some(account));
    let before = Sql::Text(Timestamp::from_unix_nanos(since.unix_nanos() - 1_000_000_000).iso());
    let latest: Option<(String, f64)> = db.query_row(&format!("SELECT ts, used_pct {filter} AND ts < ? ORDER BY ts DESC LIMIT 1"), params_from_iter(bound.iter().chain([&before])), |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
    latest.map(|(ts, used)| Ok((moment(&ts)?, used))).transpose()
}

/// The latest reading before `at` at another level than `used`: where the plateau the scan stands on
/// there most likely began.
fn last_change(db: &Connection, account: &str, at: Timestamp, used: f64) -> Result<Option<Timestamp>, Failure> {
    let (filter, bound) = readings_filter(Some(account));
    let before = Sql::Text(at.iso());
    let changed: Option<String> = db.query_row(&format!("SELECT max(ts) {filter} AND ts < ? AND used_pct IS NOT ?"), params_from_iter(bound.iter().chain([&before, &Sql::Real(used)])), |row| row.get(0))?;
    changed.map(|ts| moment(&ts)).transpose()
}

/// The first reading after `until` at another level than the reading that follows `until`: a rise or
/// a drop, so the scan has started a new plateau by then.
fn next_change(db: &Connection, account: &str, until: Timestamp) -> Result<Option<Timestamp>, Failure> {
    let (filter, bound) = readings_filter(Some(account));
    let after = Sql::Text(until.iso());
    let next: Option<f64> = db.query_row(&format!("SELECT used_pct {filter} AND ts > ? ORDER BY ts LIMIT 1"), params_from_iter(bound.iter().chain([&after])), |row| row.get(0)).optional()?;
    let Some(used) = next else { return Ok(None) };
    let changed: Option<String> = db.query_row(&format!("SELECT min(ts) {filter} AND ts > ? AND used_pct IS NOT ?"), params_from_iter(bound.iter().chain([&after, &Sql::Real(used)])), |row| row.get(0))?;
    changed.map(|ts| moment(&ts)).transpose()
}

/// Where the readings first leave their level: by a rise, or by a reset at the given index.
enum Turn {
    Rise,
    Reset(usize),
}

fn first_turn(readings: &[Reading]) -> Result<Option<Turn>, Failure> {
    for after in 1..readings.len() {
        match step(&readings[after - 1], &readings[after])? {
            Step::Level => {}
            Step::Rise(_) => return Ok(Some(Turn::Rise)),
            Step::Reset => return Ok(Some(Turn::Reset(after))),
        }
    }
    Ok(None)
}

/// The first reading after the one that follows `until` that the scan starts a new plateau at: every
/// interval from it on is priced from readings and samples later than `until` alone.
fn settled(readings: &[Reading], until: Timestamp) -> Result<Option<usize>, Failure> {
    let first = readings.partition_point(|reading| reading.at <= until) + 1;
    for after in first..readings.len() {
        if !matches!(step(&readings[after - 1], &readings[after])?, Step::Level) {
            return Ok(Some(after));
        }
    }
    Ok(None)
}

/// Each account's readings sorted by time, the last reading at a timestamp replacing earlier ones in
/// place; with `only`, that one account's readings within its range.
fn windows(db: &Connection, only: Option<(&str, Range)>) -> Result<Vec<(String, Vec<Reading>)>, Failure> {
    let (sql, bound) = readings_query(only);
    let since = only.and_then(|(_, range)| range.from);
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

/// Each account's usage samples sorted by time; with `only`, that one account's samples within its range.
fn samples(db: &Connection, only: Option<(&str, Range)>) -> Result<HashMap<String, Vec<Sample>>, Failure> {
    let mut by_account: HashMap<String, Vec<Sample>> = HashMap::new();
    let range = only.map(|(_, range)| range).unwrap_or_default();
    let mut clauses = Vec::new();
    let mut bound = Vec::new();
    if let Some((account, _)) = only {
        clauses.push("account = ?");
        bound.push(Sql::Text(account.to_owned()));
    }
    if let Some(from) = range.from {
        clauses.push("ts >= ?");
        bound.push(Sql::Text(minute_bound(from)));
    }
    if let Some(through) = range.through {
        clauses.push("ts < ?");
        bound.push(Sql::Text(minute_after(through)));
    }
    let filter = if clauses.is_empty() { String::new() } else { format!(" WHERE {}", clauses.join(" AND ")) };
    let mut query = db.prepare(&format!("SELECT account, ts, agent, weight FROM run_usage{filter} ORDER BY rowid"))?;
    for row in query.query_map(params_from_iter(bound), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?)))? {
        let (account, ts, agent, weight) = row?;
        let at = moment(&ts)?;
        if range.from.is_none_or(|from| at >= from) && range.through.is_none_or(|through| at <= through) {
            by_account.entry(account).or_default().push(Sample { at, agent, weight });
        }
    }
    for samples in by_account.values_mut() {
        samples.sort_by_key(|sample| sample.at);
    }
    Ok(by_account)
}

/// The priced intervals of every account.
fn every_interval(db: &Connection, prices: &BTreeMap<String, f64>) -> Result<Costs, Failure> {
    let mut fresh = Costs::default();
    let by_account = samples(db, None)?;
    for (account, readings) in windows(db, None)? {
        if let Some(&price) = prices.get(&account) {
            rises(&mut fresh, &account, &readings, by_account.get(&account).map_or(&[][..], Vec::as_slice), price)?;
        }
    }
    Ok(fresh)
}

/// The priced intervals of one account that end from `span.since` on, and the moment past which its
/// intervals are priced from readings and samples later than `span.until` alone, so stand as stored;
/// the intervals stop there when there is one.
fn account_intervals(db: &Connection, account: &str, span: Span, price: f64) -> Result<(Costs, Option<Timestamp>), Failure> {
    let mut fresh = Costs::default();
    let since = span.since;
    let next = next_change(db, account, span.until)?;
    let load = |from: Option<Timestamp>| -> Result<(Vec<Reading>, Option<usize>), Failure> {
        let read = |through| -> Result<Vec<Reading>, Failure> { Ok(windows(db, Some((account, Range { from, through })))?.into_iter().find(|(name, _)| name == account).map(|(_, readings)| readings).unwrap_or_default()) };
        if next.is_some() {
            let readings = read(next)?;
            if let Some(end) = settled(&readings, span.until)? {
                return Ok((readings, Some(end)));
            }
        }
        let readings = read(None)?;
        let end = settled(&readings, span.until)?;
        Ok((readings, end))
    };
    let latest = latest_before(db, account, since)?;
    let (near, end) = load(Some(latest.map_or(since, |(at, _)| at)))?;
    let near = &near[..end.map_or(near.len(), |end| end + 1)];
    let through = end.map(|_| near[near.len() - 1].at);
    let tail = near.partition_point(|reading| reading.at < since).saturating_sub(1);
    match first_turn(&near[tail..])? {
        None => return Ok((fresh, through)),
        Some(Turn::Reset(at)) => {
            let readings = &near[tail + at..];
            let by_account = samples(db, Some((account, Range { from: Some(readings[0].at), through })))?;
            rises(&mut fresh, account, readings, by_account.get(account).map_or(&[][..], Vec::as_slice), price)?;
            return Ok((fresh, through));
        }
        Some(Turn::Rise) => {}
    }
    let changed = match latest {
        Some((at, used)) => last_change(db, account, at, used)?.map(|at| Timestamp::from_unix_nanos(at.unix_nanos() - 60_000_000_000)),
        None => None,
    };
    let mut cuts: Vec<Timestamp> = changed.into_iter().collect();
    for days in [1, 4, 16, WIDEST_LOOKBACK_DAYS] {
        let cut = days_before(since, days);
        if cuts.last().is_none_or(|last| cut < *last) {
            cuts.push(cut);
        }
    }
    let (readings, start, end) = 'found: {
        for cut in cuts {
            let (readings, end) = load(Some(cut))?;
            if let Some(start) = plateau(&readings, since, false)? {
                break 'found (readings, start, end);
            }
        }
        let (readings, end) = load(None)?;
        let start = plateau(&readings, since, true)?.unwrap_or_default();
        (readings, start, end)
    };
    let readings = &readings[..end.map_or(readings.len(), |end| end + 1)];
    let Some(initial) = readings.get(start) else { return Ok((fresh, through)) };
    let by_account = samples(db, Some((account, Range { from: Some(initial.at), through })))?;
    rises(&mut fresh, account, &readings[start..], by_account.get(account).map_or(&[][..], Vec::as_slice), price)?;
    Ok((fresh, through))
}

/// Where the scan's plateau stands when it reaches the first reading at or after `since`: the first of
/// the unbroken run of level readings just before it. None when that run may reach past the first
/// loaded reading and `whole` says the readings are not the account's whole history.
fn plateau(readings: &[Reading], since: Timestamp, whole: bool) -> Result<Option<usize>, Failure> {
    let mut start = readings.partition_point(|reading| reading.at < since).saturating_sub(1);
    while start > 0 && matches!(step(&readings[start - 1], &readings[start])?, Step::Level) {
        start -= 1;
    }
    Ok((start > 0 || whole).then_some(start))
}

/// Each rise within one window, from the plateau it left to the reading that ends it, priced and
/// split by the weight each run sent after the plateau and by the reading.
fn rises(fresh: &mut Costs, account: &str, readings: &[Reading], events: &[Sample], price: f64) -> Result<(), Failure> {
    let mut plateau = 0;
    for after in 1..readings.len() {
        let later = &readings[after];
        let change = match step(&readings[after - 1], later)? {
            Step::Reset => {
                plateau = after;
                continue;
            }
            Step::Level => continue,
            Step::Rise(change) => change,
        };
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
mod tests;
