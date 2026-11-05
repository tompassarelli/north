//! What `north fleet waits` reads about a usage limit: the account's reset from the usage table, and
//! which parked session holds a lane that needs landing first.
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, params};

use crate::fleet::lane_probe::Git;
use crate::fleet::limit::AccountResets;
use crate::fleet::stranded::{Lanes, Worktree};
use crate::foundation::clock::Timestamp;
use crate::land::queue;
use crate::platform::process::run_within;
use crate::usage::{open_ledger, profiles};
use crate::watch;

const FULL_PCT: f64 = 99.5;
const QUERY: Duration = Duration::from_secs(10);

/// The reset of the window the account is out of, else of the soonest window, from the latest `usage` rows.
pub(super) struct UsageTable;

impl AccountResets for UsageTable {
    fn reset_ms(&self, account: &str, now_ms: u64) -> Option<u64> {
        let db = open_ledger(&profiles::home(), None, &|| {}).ok()?;
        window_reset_ms(&db, account, now_ms)
    }
}

fn window_reset_ms(db: &Connection, account: &str, now_ms: u64) -> Option<u64> {
    let mut query = db.prepare("SELECT name, used_pct, resets_at FROM usage WHERE provider = 'claude' AND account = ?1 AND kind = 'window' AND resets_at IS NOT NULL AND resets_at != '' ORDER BY ts DESC, rowid DESC LIMIT 400").ok()?;
    let rows = query.query_map(params![account], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?, row.get::<_, String>(2)?))).ok()?;
    let mut latest: Vec<(String, f64, u64)> = Vec::new();
    for (name, used, resets_at) in rows.flatten() {
        let at = u64::try_from(Timestamp::parse(&resets_at).ok()?.unix_millis()).ok()?;
        if !latest.iter().any(|(seen, _, _)| *seen == name) {
            latest.push((name, used.unwrap_or(0.0), at));
        }
    }
    let ahead = latest.into_iter().filter(|(_, _, at)| *at > now_ms);
    let (full, open): (Vec<_>, Vec<_>) = ahead.partition(|(_, used, _)| *used >= FULL_PCT);
    full.iter().map(|(_, _, at)| *at).max().or_else(|| open.iter().map(|(_, _, at)| *at).min())
}

/// Whether a session holds work origin/main lacks that no queued ticket will land: a lane the lander sent
/// back, or one never enqueued. Such a worker revives before the rest.
pub(super) fn holds_unqueued_lane(session: &str, cwd: &Path) -> bool {
    let tree = Worktree { path: cwd.to_owned(), branch: None };
    Git.lanes(None, &[tree]).iter().any(|lane| lane.unlanded()) && !queued(session, cwd)
}

fn queued(session: &str, cwd: &Path) -> bool {
    let cwd = cwd.to_string_lossy();
    let origin = run_within("git", &["-C", &cwd, "remote", "get-url", "--push", "origin"], QUERY).filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(origin) = origin else { return false };
    queue::pending(&queue::directory(&watch::state_home(), &origin)).is_ok_and(|tickets| tickets.iter().any(|(_, ticket)| ticket.session.as_deref() == Some(session)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT").unwrap();
        db
    }

    fn put(db: &Connection, ts: &str, name: &str, used: f64, resets_at: &str) {
        db.execute("INSERT INTO usage VALUES (?1, 'claude', 'd', 'window', ?2, ?3, 300, ?4, NULL, 'test')", params![ts, name, used, resets_at]).unwrap();
    }

    const NOW_MS: u64 = 1_793_000_000_000;

    #[test]
    fn the_window_that_is_full_names_the_reset_else_the_soonest_one_and_a_stale_row_loses_to_the_latest() {
        let db = table();
        put(&db, "2026-11-05T01:00:00Z", "session", 100.0, "2026-11-05T09:00:00Z");
        put(&db, "2026-11-05T01:00:00Z", "weekly_all", 40.0, "2026-11-09T09:00:00Z");
        let at = |text: &str| u64::try_from(Timestamp::parse(text).unwrap().unix_millis()).unwrap();
        let before = at("2026-11-05T02:00:00Z");
        assert_eq!(window_reset_ms(&db, "d", before), Some(at("2026-11-05T09:00:00Z")));
        put(&db, "2026-11-05T09:01:00Z", "session", 3.0, "2026-11-05T14:00:00Z");
        assert_eq!(window_reset_ms(&db, "d", at("2026-11-05T09:02:00Z")), Some(at("2026-11-05T14:00:00Z")));
        assert_eq!(window_reset_ms(&db, "other", NOW_MS), None);
    }
}
