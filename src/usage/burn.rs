//! The weekly window's burn rate in %/h and when it reaches 100%, or why that is unknown.
use rusqlite::{Connection, params};

use super::py;
use super::store::Row;
use crate::foundation::clock::Timestamp;

const HOURS_BACK: i64 = 3 * 3600;

pub(super) fn describe(db: &Connection, mine: &[&Row], now: Timestamp) -> Result<String, String> {
    let weekly = mine.iter().find(|row| {
        row.text("kind") == Some("window")
            && matches!(row.text("name"), Some("week" | "weekly_all"))
            && row.number("used_pct").is_some()
            && row.text("resets_at").is_some_and(|text| !text.is_empty())
            && row.number("window_min").is_some_and(|minutes| minutes != 0.0)
    });
    let Some(row) = weekly else { return Ok("burn unknown (no weekly window)".into()) };
    let (resets_at, used) = (row.text("resets_at").unwrap_or_default(), row.number("used_pct").unwrap_or_default());
    let readings: i64 = db
        .query_row(
            "SELECT count(*) FROM usage WHERE provider = ? AND account = ? AND name = ? AND kind = 'window' \
             AND abs(julianday(resets_at) - julianday(?)) * 86400 < 120",
            params![row.text("provider"), row.text("account"), row.text("name"), resets_at],
            |found| found.get(0),
        )
        .map_err(|error| error.to_string())?;
    let count = format!("{readings} reading{}", if readings == 1 { "" } else { "s" });
    if readings < 2 {
        return Ok(format!("burn unknown ({count})"));
    }
    let Some(rate) = beta(db, row, now)? else {
        return Ok(format!("burn unknown (need two readings in last 3h; {count})"));
    };
    let reset = py::parse(resets_at)?;
    let empty = if rate > 0.0 { py::after_hours(now, (100.0 - used).max(0.0) / rate) } else { reset };
    if empty >= reset {
        return Ok(format!("burn {rate:.1}%/h, lasts to reset ({}; {count})", py::until(Some(resets_at), now)?));
    }
    let moment = py::civil(empty);
    Ok(format!(
        "burn {rate:.1}%/h, empty in {} ({} {} {:02}:{:02}Z; {count})",
        py::until(Some(&py::second(empty)), now)?,
        moment.day(),
        py::month(&moment),
        moment.hour(),
        moment.minute()
    ))
}

pub(super) fn beta(db: &Connection, row: &Row, now: Timestamp) -> Result<Option<f64>, String> {
    let resets_at = row.text("resets_at").unwrap_or_default();
    let window = i64::from(row.number("window_min").unwrap_or_default() as i32) * 60;
    let since = std::cmp::max(py::before(py::parse(resets_at)?, window), py::before(now, HOURS_BACK));
    let old: Option<(String, f64)> = db
        .query_row(
            "SELECT ts, used_pct FROM usage WHERE provider = ? AND account = ? AND kind = 'window' AND name = ? AND resets_at = ? \
             AND ts >= ? AND ts < ? AND used_pct IS NOT NULL ORDER BY ts LIMIT 1",
            params![row.text("provider"), row.text("account"), row.text("name"), resets_at, py::isoformat(since), row.text("ts")],
            |found| Ok((found.get(0)?, found.get(1)?)),
        )
        .ok();
    let Some((then, was)) = old else { return Ok(None) };
    let hours = (py::parse(row.text("ts").unwrap_or_default())?.unix_nanos() - py::parse(&then)?.unix_nanos()) as f64 / 1e9 / 3600.0;
    Ok((hours >= 1.0 / 6.0).then(|| (row.number("used_pct").unwrap_or_default() - was).max(0.0) / hours))
}
