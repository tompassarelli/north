//! A repository's landing tempo from the `landings`, `land_checks` and `main_red` tables.
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

const WINDOW: f64 = 86_400.0;
const DAY: i64 = 86_400;

struct Landing {
    lane: String,
    candidate: String,
    batch_size: i64,
    bisect_depth: i64,
    queued_at: Option<f64>,
    landed_at: Option<f64>,
    landed: bool,
    first_commit_at: Option<f64>,
    check_started_at: Option<f64>,
    check_finished_at: Option<f64>,
}

fn seconds(text: &str) -> Option<f64> {
    Timestamp::parse_python_datetime(text).map(Timestamp::unix_seconds_f64)
}

pub(crate) fn percentiles(samples: &[f64]) -> (Option<f64>, Option<f64>) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let length = sorted.len();
    if length == 0 {
        return (None, None);
    }
    let middle = if length % 2 == 1 { sorted[length / 2] } else { (sorted[length / 2 - 1] + sorted[length / 2]) / 2.0 };
    (Some(middle), Some(sorted[(length as f64 * 0.9).ceil() as usize - 1]))
}

fn minutes_between(rows: &[&Landing], from: impl Fn(&Landing) -> Option<f64>, to: impl Fn(&Landing) -> Option<f64>) -> Vec<f64> {
    rows.iter().filter_map(|row| Some((to(row)? - from(row)?).max(0.0) / 60.0)).collect()
}

fn column(db: &Connection, name: &'static str) -> rusqlite::Result<&'static str> {
    let present: bool = db.query_row("SELECT count(*) > 0 FROM pragma_table_info('landings') WHERE name = ?1", [name], |row| row.get(0))?;
    Ok(if present { name } else { "NULL" })
}

fn landings(db: &Connection, repo: &str, since: &str) -> rusqlite::Result<Vec<Landing>> {
    let (first, started, finished) = (column(db, "first_commit_at")?, column(db, "check_started_at")?, column(db, "check_finished_at")?);
    let mut statement = db.prepare(&format!(
        "SELECT lane, candidate, batch_size, bisect_depth, queued_at, landed_at, verdict = 'landed', {first}, {started}, {finished} FROM landings \
         WHERE repo = ?1 AND coalesce(landed_at, queued_at) >= ?2 ORDER BY queued_at",
    ))?;
    let rows = statement.query_map([repo, since], |row| {
        Ok(Landing {
            lane: row.get(0)?,
            candidate: row.get(1)?,
            batch_size: row.get(2)?,
            bisect_depth: row.get(3)?,
            queued_at: row.get::<_, String>(4).ok().and_then(|text| seconds(&text)),
            landed_at: row.get::<_, Option<String>>(5)?.and_then(|text| seconds(&text)),
            landed: row.get(6)?,
            first_commit_at: row.get::<_, Option<String>>(7)?.and_then(|text| seconds(&text)),
            check_started_at: row.get::<_, Option<String>>(8)?.and_then(|text| seconds(&text)),
            check_finished_at: row.get::<_, Option<String>>(9)?.and_then(|text| seconds(&text)),
        })
    })?;
    rows.collect()
}

fn re_lands(rows: &[Landing]) -> (usize, usize) {
    let mut lanes = std::collections::BTreeSet::new();
    let mut count = 0;
    for lane in rows.iter().filter(|row| row.landed).map(|row| &row.lane).collect::<std::collections::BTreeSet<_>>() {
        let earlier = rows.iter().filter(|row| &row.lane == lane && !row.landed).count();
        if earlier > 0 {
            lanes.insert(lane);
            count += earlier;
        }
    }
    (count, lanes.len())
}

fn mean_batch(rows: &[&Landing]) -> Option<f64> {
    let mut batches = std::collections::BTreeMap::new();
    for row in rows.iter().filter(|row| row.batch_size > 0) {
        batches.insert(&row.candidate, row.batch_size);
    }
    (!batches.is_empty()).then(|| batches.values().sum::<i64>() as f64 / batches.len() as f64)
}

fn checks(db: &Connection, repo: &str, since: &str) -> rusqlite::Result<(i64, i64)> {
    db.query_row("SELECT count(*), coalesce(sum(outcome = 'moved'), 0) FROM land_checks WHERE repo = ?1 AND started_at >= ?2", [repo, since], |row| Ok((row.get(0)?, row.get(1)?)))
}

fn red_minutes(db: &Connection, repo: &str, now: f64) -> rusqlite::Result<f64> {
    let day_start = (now as i64).div_euclid(DAY) as f64 * DAY as f64;
    let mut statement = db.prepare("SELECT red_at, green_at FROM main_red WHERE repo = ?1")?;
    let intervals = statement.query_map([repo], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))?;
    let mut total = 0.0;
    for (red, green) in intervals.collect::<rusqlite::Result<Vec<_>>>()? {
        let (Some(start), end) = (seconds(&red), green.as_deref().and_then(seconds).unwrap_or(now)) else { continue };
        total += (end.min(now) - start.max(day_start)).max(0.0);
    }
    Ok(total / 60.0)
}

fn read(db: &Connection, repo: &str, now: f64) -> rusqlite::Result<Option<Value>> {
    let since = Timestamp::from_unix_seconds_f64(now - WINDOW).iso();
    let rows = landings(db, repo, &since)?;
    let (runs, thrown_away) = checks(db, repo, &since)?;
    let red = red_minutes(db, repo, now)?;
    if rows.is_empty() && runs == 0 && red == 0.0 {
        return Ok(None);
    }
    let landed: Vec<&Landing> = rows.iter().filter(|row| row.landed).collect();
    let waits: Vec<f64> = landed.iter().filter(|row| row.batch_size > 0).filter_map(|row| Some((row.landed_at? - row.queued_at?) / 60.0)).collect();
    let (median, p90) = percentiles(&waits);
    let first_to_landed = minutes_between(&landed, |row| row.first_commit_at, |row| row.landed_at);
    let before_queue = minutes_between(&landed, |row| row.first_commit_at, |row| row.queued_at);
    let queued = minutes_between(&landed, |row| row.queued_at, |row| row.check_started_at);
    let checked = minutes_between(&landed, |row| row.check_started_at, |row| row.check_finished_at);
    let (first_median, first_p90) = percentiles(&first_to_landed);
    let refusals = rows.iter().filter(|row| !row.landed).count();
    let (relands, relanded_lanes) = re_lands(&rows);
    Ok(Some(json!({
        "window_hours": WINDOW / 3600.0,
        "landings": landed.len(),
        "landings_per_hour": landed.len() as f64 / (WINDOW / 3600.0),
        "queue_to_landed_p50_minutes": median,
        "queue_to_landed_p90_minutes": p90,
        "first_commit_to_landed_p50_minutes": first_median,
        "first_commit_to_landed_p90_minutes": first_p90,
        "first_commit_samples": first_to_landed.len(),
        "before_queue_p50_minutes": percentiles(&before_queue).0,
        "queued_p50_minutes": percentiles(&queued).0,
        "check_p50_minutes": percentiles(&checked).0,
        "refusals": refusals,
        "refusal_share": (!rows.is_empty()).then(|| refusals as f64 / rows.len() as f64),
        "mean_batch_size": mean_batch(&landed),
        "check_runs": runs,
        "checks_thrown_away": thrown_away,
        "thrown_away_share": (runs > 0).then(|| thrown_away as f64 / runs as f64),
        "re_lands": relands,
        "re_landed_lanes": relanded_lanes,
        "deepest_bisect": rows.iter().map(|row| row.bisect_depth).max(),
        "red_main_minutes_today": red,
    })))
}

/// The repository's tempo over the last 24 hours; `None` when the ledger holds nothing for it.
pub fn tempo(ledger: &Path, repo: &str, now: f64) -> Option<Value> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    read(&db, repo, now).ok().flatten()
}

fn figure(value: &Value, digits: usize, unit: &str) -> String {
    value.as_f64().map_or_else(|| "n/a".into(), |number| format!("{number:.digits$}{unit}"))
}

/// The one line `north flow` prints for a repository's tempo.
pub fn line(repo: &str, tempo: &Value) -> String {
    let share = tempo["thrown_away_share"].as_f64().map_or_else(|| "n/a".into(), |share| format!("{:.0}%", share * 100.0));
    let refused = tempo["refusal_share"].as_f64().map_or_else(|| "n/a".into(), |share| format!("{:.0}%", share * 100.0));
    format!(
        "landing {repo}: {} landings, {} per hour over {:.0}h, finished-to-landed p50 {} p90 {}, first-commit-to-landed p50 {} p90 {} over {} landings (before queue p50 {}, queued p50 {}, check p50 {}), refusals {} = {refused}, mean batch {}, re-lands {} across {} lanes, checks thrown away (main moved) {}/{} = {share}, deepest bisect {}, red main today {}",
        tempo["landings"],
        figure(&tempo["landings_per_hour"], 2, ""),
        tempo["window_hours"].as_f64().unwrap_or(0.0),
        figure(&tempo["queue_to_landed_p50_minutes"], 1, "m"),
        figure(&tempo["queue_to_landed_p90_minutes"], 1, "m"),
        figure(&tempo["first_commit_to_landed_p50_minutes"], 1, "m"),
        figure(&tempo["first_commit_to_landed_p90_minutes"], 1, "m"),
        tempo["first_commit_samples"],
        figure(&tempo["before_queue_p50_minutes"], 1, "m"),
        figure(&tempo["queued_p50_minutes"], 1, "m"),
        figure(&tempo["check_p50_minutes"], 1, "m"),
        tempo["refusals"],
        figure(&tempo["mean_batch_size"], 1, ""),
        tempo["re_lands"],
        tempo["re_landed_lanes"],
        tempo["checks_thrown_away"],
        tempo["check_runs"],
        tempo["deepest_bisect"].as_i64().map_or_else(|| "n/a".into(), |depth| depth.to_string()),
        figure(&tempo["red_main_minutes_today"], 0, "m"),
    )
}
