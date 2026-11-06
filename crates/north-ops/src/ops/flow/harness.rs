//! The harness cause classes that lost the most worker runs over the last 24 h, with the API-equivalent usage each
//! cost: the order the proxy staffs harness fixes in.
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::threads::calibration::attribution::{Attributed, attribute, pareto};

const DAY_S: f64 = 86_400.0;
const SHOWN: usize = 5;

/// `harness causes, 24 h: landing 12 runs $84, ... (17 of 60 attributed runs, $120)`.
pub fn line(runs: &[Attributed]) -> String {
    let ranked = pareto(runs);
    let judged = runs.iter().filter(|run| run.cause.is_some()).count();
    let (lost, usd) = ranked.iter().fold((0, 0.0), |(count, usd), (_, runs, cost)| (count + runs, usd + cost));
    if ranked.is_empty() {
        return format!("harness causes, 24 h: none in {judged} attributed runs");
    }
    let top: Vec<String> = ranked.iter().take(SHOWN).map(|(class, count, cost)| format!("{class} {count} runs ${cost:.0}")).collect();
    format!("harness causes, 24 h: {} ({lost} of {judged} attributed runs, ${usd:.0})", top.join(", "))
}

pub fn read(ledger: &Path, now: f64) -> Result<String, String> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("{}: {why}", ledger.display()))?;
    attribute(&db, now - DAY_S, now + 1.0).map(|runs| line(&runs)).map_err(|why| why.to_string())
}
