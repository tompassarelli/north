use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::Connection;
use rusqlite::types::Value as Cell;

use crate::foundation::clock::Timestamp;

use super::super::tiers::Tier;

const RUN_TIERS: [(&str, &str); 10] = [
    ("claude:haiku", "haiku"),
    ("claude:sonnet-high", "sonnet"),
    ("claude:opus-medium", "medium"),
    ("claude:opus-high", "high"),
    ("claude:opus-xhigh", "xhigh"),
    ("codex:luna-low", "gpt-6-luna low"),
    ("codex:sol-medium", "gpt-6.1-sol medium"),
    ("codex:sol-high", "gpt-6.1-sol high"),
    ("codex:sol-xhigh", "gpt-6.1-sol xhigh"),
    ("codex:astra", "gpt-6-astra xhigh"),
];

pub(super) struct Ledger(pub Option<Connection>);

pub(super) fn run_tier(tier: &Tier) -> String {
    RUN_TIERS
        .iter()
        .find(|(key, _)| *key == tier.key)
        .map_or_else(|| format!("{} {}", tier.model, tier.effort), |(_, name)| (*name).to_owned())
}

pub(super) fn instant(text: &str) -> Option<Timestamp> {
    Timestamp::parse_rfc3339(text).or_else(|| Timestamp::parse(text).ok())
}

pub(super) fn shifted(time: Timestamp, seconds: f64) -> Timestamp {
    let nanos = (time.unix_nanos() as f64 + seconds * 1e9).max(0.0) as u64;
    Timestamp::from(UNIX_EPOCH + Duration::from_nanos(nanos))
}

fn text(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Text(text) => Some(text.clone()),
        Cell::Integer(number) => Some(number.to_string()),
        Cell::Real(number) => Some(number.to_string()),
        _ => None,
    }
}

fn truthy(cell: &Cell) -> bool {
    match cell {
        Cell::Integer(number) => *number != 0,
        Cell::Real(number) => *number != 0.0,
        Cell::Text(text) => !text.is_empty(),
        _ => false,
    }
}

impl Ledger {
    pub(super) fn query(&self, sql: &str, parameters: &[&dyn rusqlite::ToSql]) -> Vec<Vec<Cell>> {
        let Some(db) = &self.0 else {
            return Vec::new();
        };
        let rows = db.prepare(sql).and_then(|mut statement| {
            let width = statement.column_count();
            statement
                .query_map(parameters, |row| (0..width).map(|index| row.get::<_, Cell>(index)).collect::<Result<Vec<_>, _>>())
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        });
        rows.unwrap_or_default()
    }

    pub(super) fn measured(&self, tiers: &[Tier], min_runs: usize) -> (BTreeMap<String, i64>, BTreeMap<String, usize>) {
        let keys: BTreeMap<String, &str> = tiers.iter().map(|tier| (run_tier(tier), tier.key.as_str())).collect();
        let escalated: BTreeSet<String> = self
            .query("SELECT follows FROM runs WHERE role = 'worker' AND follows IS NOT NULL", &[])
            .iter()
            .filter_map(|row| text(&row[0]))
            .collect();
        let mut runs: BTreeMap<String, usize> = tiers.iter().map(|tier| (tier.key.clone(), 0)).collect();
        let mut cells: BTreeMap<String, Vec<bool>> = BTreeMap::new();
        let sql = "SELECT agent, tier, category, outcome, expects_landing, landed, item FROM runs \
                   WHERE role = 'worker' AND follows IS NULL AND outcome != 'split'";
        for row in self.query(sql, &[]) {
            let outcome = text(&row[3]).unwrap_or_default();
            let item = truthy(&row[6]);
            let unjudged = outcome == "running" || outcome == "idle" || (outcome == "unclear" && !item);
            let Some(key) = text(&row[1]).and_then(|tier| keys.get(&tier).copied()) else {
                continue;
            };
            if unjudged {
                continue;
            }
            *runs.entry(key.to_owned()).or_default() += 1;
            let followed = text(&row[0]).is_some_and(|agent| escalated.contains(&agent));
            let landed_zero = matches!(&row[5], Cell::Integer(0)) || matches!(&row[5], Cell::Real(value) if *value == 0.0);
            let succeeded = outcome == "done" && !followed && !(truthy(&row[4]) && landed_zero);
            let category = text(&row[2]).unwrap_or_else(|| "None".to_owned());
            cells.entry(format!("{key}|{category}")).or_default().push(succeeded);
        }
        let success = cells
            .into_iter()
            .filter(|(_, outcomes)| outcomes.len() >= min_runs)
            .map(|(cell, outcomes)| (cell, (outcomes.iter().filter(|ok| **ok).count() * 100 / outcomes.len()) as i64))
            .collect();
        (success, runs)
    }
}

pub(super) fn python_iso(time: Timestamp) -> String {
    let text = time.iso_micros_utc_offset();
    text.strip_suffix("+00:00").map_or(text.clone(), |stem| stem.strip_suffix(".000000").unwrap_or(stem).to_owned() + "Z")
}
