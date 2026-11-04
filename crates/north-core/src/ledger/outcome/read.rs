use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use super::attention::{Bug, Correction, Typed, detector, messages};
use super::{Landed, Staffed, WEEK_S, join, lines, weeks};
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;
use crate::router::pricing::PriceTable;
use crate::threads::calibration::rework::{Rework, Sources};

pub const LOOKBACK_S: i64 = 3 * WEEK_S;

#[derive(Clone, Debug)]
pub struct Places {
    pub ledger: PathBuf,
    pub convo: PathBuf,
    pub grounding: PathBuf,
    pub rework: Sources,
}

impl Places {
    pub fn live(ledger: &Path, state: &Path) -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self {
            ledger: ledger.to_path_buf(),
            convo: var("CONVO_STATE").unwrap_or_else(|| state.join("convo")).join("index-v4.db"),
            grounding: var("NORTH_GROUNDING_STATE").unwrap_or_else(|| state.join("north/grounding")).join("ledger.jsonl"),
            rework: Sources::live(),
        }
    }
}

fn seconds(text: Option<String>) -> Option<i64> {
    text.filter(|text| !text.is_empty()).and_then(|text| Timestamp::parse_python_datetime(&text)).map(Timestamp::unix_seconds)
}

fn table_has(db: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) > 0 FROM pragma_table_info(?1) WHERE name = ?2", [table, column], |row| row.get(0))
}

pub fn staffed(db: &Connection, prices: &PriceTable) -> rusqlite::Result<Vec<Staffed>> {
    let mut statement = db.prepare(&format!(
        "SELECT agent, tier, coalesce(arm, ''), ended, landed_sha, landed_at, first_commit, model, coalesce(input_tokens, 0), coalesce(cache_read_tokens, 0), coalesce(tokens, 0) \
         FROM runs WHERE {} AND outcome != 'running' AND ended != '' ORDER BY ended, agent",
        worker_where()
    ))?;
    let rows = statement.query_map([], |row| {
        let tier: String = row.get(1)?;
        let model: Option<String> = row.get(7)?;
        Ok(Staffed {
            agent: row.get(0)?,
            arm: row.get(2)?,
            ended: seconds(row.get(3)?).unwrap_or(0),
            landed_sha: row.get(4)?,
            landed_at: seconds(row.get(5)?),
            first_commit: seconds(row.get(6)?),
            usd: prices.run_usd(model.as_deref(), &tier, row.get(8)?, row.get(9)?, row.get(10)?),
            tier,
        })
    })?;
    rows.collect()
}

pub fn landed(db: &Connection) -> rusqlite::Result<Vec<Landed>> {
    if !table_has(db, "landings", "landed_sha")? {
        return Ok(Vec::new());
    }
    let first = if table_has(db, "landings", "first_commit_at")? { "first_commit_at" } else { "NULL" };
    let mut statement = db.prepare(&format!("SELECT landed_sha, landed_at, {first} FROM landings WHERE verdict = 'landed' AND coalesce(landed_sha, '') != '' ORDER BY queued_at"))?;
    let rows = statement.query_map([], |row| Ok(Landed { sha: row.get(0)?, landed_at: seconds(row.get(1)?), first_commit_at: seconds(row.get(2)?) }))?;
    rows.collect()
}

pub fn bugs(db: &Connection) -> rusqlite::Result<Vec<Bug>> {
    let mut statement = db.prepare(&format!(
        "SELECT item, min(coalesce(nullif(staffed, ''), nullif(started, ''), ended)) FROM runs WHERE {} AND lower(category) LIKE 'bug%' AND coalesce(item, '') != '' GROUP BY item ORDER BY item",
        worker_where()
    ))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, seconds(row.get(1)?))))?;
    Ok(rows.filter_map(Result::ok).filter_map(|(item, at)| Some(Bug { item, at: at? })).collect())
}

pub fn alerts(path: &Path, since: i64) -> rusqlite::Result<Vec<(String, i64, String)>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let mut statement = db.prepare(
        "SELECT coalesce(f.session_id, f.path), m.ts, b.text FROM files f JOIN msg m ON m.file_id = f.id JOIN body b ON b.id = m.body \
         WHERE f.agent IS NULL AND f.last_ts >= ?1 AND m.role = 'user' AND m.tool IS NULL AND m.dup = 0 ORDER BY f.id, m.line",
    )?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, seconds(row.get(1)?), row.get::<_, String>(2)?)))?;
    Ok(rows.filter_map(Result::ok).filter(|(_, _, text)| detector(text)).filter_map(|(session, at, text)| Some((session, at?, text))).collect())
}

#[derive(Deserialize)]
struct GroundingRow {
    at: String,
    kind: String,
    quote: String,
    source: String,
}

pub fn corrections(path: &Path, tom: &[Typed]) -> Vec<Correction> {
    let typed: BTreeSet<i64> = tom.iter().map(|typed| typed.id).collect();
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .filter_map(|line| serde_json::from_str::<GroundingRow>(line).ok())
        .filter_map(|row| Correction::from_row(seconds(Some(row.at))?, &row.kind, &row.source, &row.quote, &typed))
        .collect()
}

pub fn report(places: &Places, tom: &[Typed], now: i64) -> Result<Vec<String>, String> {
    let db = Connection::open_with_flags(&places.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("ledger {}: {why}", places.ledger.display()))?;
    let prices = PriceTable::load().map_err(|why| format!("prices: {why}"))?;
    let failed = |why: rusqlite::Error| format!("ledger {}: {why}", places.ledger.display());
    let rework = Rework::read(&db, &places.rework).map_err(failed)?;
    let outcomes = join(&staffed(&db, &prices).map_err(failed)?, &landed(&db).map_err(failed)?, &rework.judged);
    let delivered = alerts(&places.convo, now - LOOKBACK_S).map_err(|why| format!("convo store {}: {why}", places.convo.display()))?;
    let messages = messages(tom, &delivered);
    let sources = format!(
        "runs and landings in {}; rework from each ~/code/<repo>/main history within 48 h; cost API-equivalent from router/prices.json; Tom's messages are the human turns of his own conversations (convo store {}, transcript origin human, as presence reads them); corrections from {} (tom correction and tom direction rows whose message is one of those turns); detectors are alerts in the convo store opening {}",
        places.ledger.display(),
        places.convo.display(),
        places.grounding.display(),
        super::attention::DETECTORS.join(", "),
    );
    Ok(lines(&weeks(&outcomes, &messages, &corrections(&places.grounding, tom), &bugs(&db).map_err(failed)?, now), &sources))
}
