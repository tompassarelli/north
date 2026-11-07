//! One account's weekly usage window as the ledger's readouts sampled it.
use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

const NAMES: &str = "('week', 'weekly_all')";
const WEEK_S: i64 = 7 * 86_400;
const HOUR_S: i64 = 3_600;
const FULL: f64 = 100.0;

pub(super) struct Week {
    /// When the window resets, in unix seconds.
    pub(super) resets: i64,
    /// The readouts of this window as (time, used percent), oldest first.
    pub(super) readings: Vec<(i64, f64)>,
}

impl Week {
    /// The percent used by `at`: the highest readout at or before it, so a lower readout from another source never counts as a drop and a refill.
    fn used_by(&self, at: i64) -> f64 {
        let held = self.readings.iter().take_while(|(time, _)| *time <= at).map(|(_, used)| *used).fold(None, |most: Option<f64>, used| Some(most.map_or(used, |most| most.max(used))));
        match (held, self.readings.first()) {
            (Some(used), _) => used,
            (None, Some(_)) if at < self.resets - WEEK_S => 0.0,
            (None, Some((_, first))) => *first,
            (None, None) => 0.0,
        }
    }

    /// The percent of the window used in `start..end`.
    pub fn spent(&self, start: i64, end: i64) -> f64 {
        (self.used_by(end - 1) - self.used_by(start - 1)).max(0.0)
    }

    /// The percent left when the window reset.
    pub fn unspent(&self) -> f64 {
        (FULL - self.used_by(self.resets)).max(0.0)
    }
}

/// Every Claude account's weekly windows with readouts since `since`, a window keyed by its account and its reset to the hour.
pub(super) fn load(db: &Connection, since: i64) -> rusqlite::Result<Vec<Week>> {
    let mut statement = db.prepare(&format!("SELECT account, ts, used_pct, resets_at FROM usage WHERE provider = 'claude' AND kind = 'window' AND name IN {NAMES} AND used_pct IS NOT NULL AND resets_at IS NOT NULL AND ts >= ?1 ORDER BY ts"))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?, row.get::<_, String>(3)?)))?;
    let mut weeks: BTreeMap<(String, i64), Week> = BTreeMap::new();
    for (account, ts, used, resets) in rows.filter_map(Result::ok) {
        let (Ok(at), Ok(resets)) = (Timestamp::parse(&ts), Timestamp::parse(&resets)) else { continue };
        let (at, resets) = (at.unix_seconds(), resets.unix_seconds());
        let resets = (resets + HOUR_S / 2) / HOUR_S * HOUR_S;
        weeks.entry((account.unwrap_or_default(), resets)).or_insert_with(|| Week { resets, readings: Vec::new() }).readings.push((at, used));
    }
    Ok(weeks.into_values().collect())
}
