//! Usage readouts into the ledger, and each run's dollar cost: every rise of an account's weekly
//! window, priced by the operator's costs.toml, split across the runs that sent requests during it.
use std::collections::{BTreeMap, HashMap};

use rusqlite::{Connection, params, params_from_iter};

use super::row::Row;
use crate::foundation::clock::Timestamp;
use crate::platform::usage_table;
use crate::threads::verb::{Failure, immediate};

const COLUMNS: [&str; 10] = ["ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source"];
const COST_WINDOWS: [&str; 3] = ["week", "weekly_all", "Gemini Models/weekly"];
const SAME_WINDOW_NANOS: i128 = 60_000_000_000;

/// Appends each readout no earlier readout of the same window at the same time recorded.
pub(crate) fn record(db: &mut Connection, readouts: &[Row]) -> Result<(), Failure> {
    usage_table::ensure(db)?;
    let tx = db.transaction()?;
    {
        let same = COLUMNS[..5].iter().map(|column| format!("{column} IS ?")).collect::<Vec<_>>().join(" AND ");
        let mut insert = tx.prepare(&format!("INSERT INTO usage({}) SELECT {} WHERE NOT EXISTS (SELECT 1 FROM usage WHERE {same})", COLUMNS.join(", "), ["?"; 10].join(", ")))?;
        for readout in readouts {
            let values = COLUMNS.iter().chain(&COLUMNS[..5]).map(|column| readout.sql(column)).collect::<Result<Vec<_>, _>>()?;
            insert.execute(params_from_iter(values))?;
        }
    }
    Ok(tx.commit()?)
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

/// Rewrites run_costs from every readout and usage sample, then each run's cost_usd from them,
/// touching only rows whose value changed.
pub(crate) fn allocate(db: &mut Connection, prices: &BTreeMap<String, f64>) -> Result<(), Failure> {
    let fresh = intervals(db, prices)?;
    let tx = immediate(db)?;
    let stored: Vec<Key> = tx.prepare("SELECT account, started, ended, agent FROM run_costs")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?.collect::<Result<_, _>>()?;
    for (account, started, ended, agent) in stored.iter().filter(|key| !fresh.index.contains_key(*key)) {
        tx.execute("DELETE FROM run_costs WHERE (account, started, ended, agent) = (?,?,?,?)", params![account, started, ended, agent])?;
    }
    for ((account, started, ended, agent), usd) in &fresh.order {
        tx.execute(
            "INSERT INTO run_costs(account, started, ended, agent, cost_usd) VALUES (?,?,?,?,?) ON CONFLICT(account, started, ended, agent) DO UPDATE SET cost_usd = excluded.cost_usd WHERE cost_usd IS NOT excluded.cost_usd",
            params![account, started, ended, agent, usd],
        )?;
    }
    tx.execute("UPDATE runs SET cost_usd = (SELECT SUM(cost_usd) FROM run_costs WHERE agent = runs.agent) WHERE cost_usd IS NOT (SELECT SUM(cost_usd) FROM run_costs WHERE agent = runs.agent)", [])?;
    Ok(tx.commit()?)
}

fn moment(text: &str) -> Result<Timestamp, Failure> {
    Timestamp::parse_python_datetime(text).ok_or_else(|| Failure::Refused(format!("unreadable usage timestamp {text}")))
}

fn same_window(a: Option<&str>, b: Option<&str>) -> Result<bool, Failure> {
    match (a, b) {
        _ if a == b => Ok(true),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Ok((moment(a)?.unix_nanos() - moment(b)?.unix_nanos()).abs() <= SAME_WINDOW_NANOS),
        _ => Ok(false),
    }
}

/// Each account's readings in first-seen order, the last reading at a timestamp replacing earlier ones in place.
fn windows(db: &Connection) -> Result<Vec<(String, Vec<Reading>)>, Failure> {
    let mut query = db.prepare("SELECT provider, account, ts, used_pct, resets_at FROM usage WHERE kind = 'window' AND used_pct IS NOT NULL AND name IN (?, ?, ?)")?;
    let rows = query.query_map(COST_WINDOWS, |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?, row.get::<_, Option<String>>(4)?)))?;
    let mut accounts: Vec<(String, Vec<Reading>)> = Vec::new();
    for row in rows {
        let (provider, account, ts, used, resets) = row?;
        let account = match provider.as_str() {
            "claude" => format!("claude-{}", account.ok_or_else(|| Failure::Refused("a claude usage readout has no account".into()))?),
            _ => provider,
        };
        let index = accounts.iter().position(|(name, _)| *name == account).unwrap_or_else(|| {
            accounts.push((account, Vec::new()));
            accounts.len() - 1
        });
        let reading = Reading { at: moment(&ts)?, ts, used, resets };
        let readings = &mut accounts[index].1;
        match readings.iter_mut().find(|earlier| earlier.ts == reading.ts) {
            Some(earlier) => *earlier = reading,
            None => readings.push(reading),
        }
    }
    Ok(accounts)
}

fn samples(db: &Connection) -> Result<HashMap<String, Vec<Sample>>, Failure> {
    let mut by_account: HashMap<String, Vec<Sample>> = HashMap::new();
    let mut query = db.prepare("SELECT account, ts, agent, weight FROM run_usage")?;
    for row in query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?)))? {
        let (account, ts, agent, weight) = row?;
        by_account.entry(account).or_default().push(Sample { at: moment(&ts)?, agent, weight });
    }
    for samples in by_account.values_mut() {
        samples.sort_by_key(|sample| sample.at);
    }
    Ok(by_account)
}

/// Each rise within one window, from the plateau it left to the reading that ends it, priced and
/// split by the weight each run sent after the plateau and by the reading.
fn intervals(db: &Connection, prices: &BTreeMap<String, f64>) -> Result<Costs, Failure> {
    let by_account = samples(db)?;
    let mut fresh = Costs::default();
    for (account, mut readings) in windows(db)? {
        readings.sort_by_key(|reading| reading.at);
        let events = by_account.get(&account).map_or(&[][..], Vec::as_slice);
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
            if let Some(price) = prices.get(&account) {
                let initial = &readings[plateau];
                let first = events.partition_point(|sample| sample.at <= initial.at);
                let last = events.partition_point(|sample| sample.at <= later.at).max(first);
                split(&mut fresh, (&account, &initial.ts, &later.ts), change * price / 100.0, &events[first..last]);
            }
            plateau = after;
        }
    }
    Ok(fresh)
}

fn split(fresh: &mut Costs, (account, started, ended): (&str, &str, &str), cost: f64, events: &[Sample]) {
    let mut weights: Vec<(&str, f64)> = Vec::new();
    for sample in events {
        match weights.iter_mut().find(|(agent, _)| *agent == sample.agent) {
            Some((_, weight)) => *weight += sample.weight,
            None => weights.push((&sample.agent, 0.0 + sample.weight)),
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
