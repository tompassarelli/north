//! One account's weekly usage window as the ledger's readouts sampled it.
use std::collections::BTreeMap;

use rusqlite::Connection;

use north_foundation::foundation::clock::Timestamp;

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
    /// The window's readouts up to `at` as the peaks of the segments that a readout of 0 ended and the highest readout of the open segment, so a lower readout from another source never counts as a drop and a refill.
    fn tally(&self, at: i64) -> Option<(f64, f64)> {
        let mut ended = 0.0;
        let mut open: Option<f64> = None;
        for (_, used) in self.readings.iter().take_while(|(time, _)| *time <= at) {
            open = Some(match open {
                Some(most) if *used <= 0.0 && most > 0.0 => {
                    ended += most;
                    *used
                }
                Some(most) => most.max(*used),
                None => *used,
            });
        }
        open.map(|open| (ended, open))
    }

    /// The percent used by `at`: the peaks of the ended segments plus the open segment's peak.
    fn used_by(&self, at: i64) -> f64 {
        match (self.tally(at), self.readings.first()) {
            (Some((ended, open)), _) => ended + open,
            (None, Some(_)) if at < self.resets - WEEK_S => 0.0,
            (None, Some((_, first))) => *first,
            (None, None) => 0.0,
        }
    }

    /// The percent of the window used in `start..end`.
    pub fn spent(&self, start: i64, end: i64) -> f64 {
        (self.used_by(end - 1) - self.used_by(start - 1)).max(0.0)
    }

    /// The percent left when the window reset: an early reset gave the window a fresh allowance, so only the open segment counts.
    pub fn unspent(&self) -> f64 {
        let open = self.tally(self.resets).map_or_else(|| self.used_by(self.resets), |(_, open)| open);
        (FULL - open).max(0.0)
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

#[cfg(test)]
mod tests {
    use super::{WEEK_S, Week};

    #[test]
    fn usage_yield_counts_spend_across_an_early_reset() {
        let week = Week { resets: WEEK_S, readings: vec![(1_000, 0.0), (2_000, 10.0), (3_000, 20.0), (4_000, 0.0), (5_000, 5.0), (6_000, 15.0)] };
        assert_eq!(week.spent(0, 10_000), 35.0);
    }
}
