//! What `north fleet waits` reads about a usage limit: the account's reset from the usage table, and
//! which parked session holds a lane that needs landing first.
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, params};

use crate::fleet::lane_probe::Git;
use crate::fleet::handover::{AccountReadings, Observed};
use crate::fleet::limit::{AccountResets, FULL_PCT};
use crate::fleet::stranded::{Lanes, Worktree};
use crate::foundation::clock::Timestamp;
use crate::land::queue;
use crate::platform::process::run_within;
use crate::usage::decide::world::{Label, LimitEvent, ModelFamily, Percent, Reading, RowId, Source, Unix, Window};
use crate::usage::{open_ledger, profiles};
use crate::watch;

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

/// The latest reading of every window of every account, with each window that reads full until a reset still
/// ahead also as a limit event, which stays true however old the reading grows.
impl AccountReadings for UsageTable {
    fn observed(&self, now_ms: u64) -> Observed {
        let Ok(db) = open_ledger(&profiles::home(), None, &|| {}) else { return Observed::default() };
        latest_windows(&db, now_ms)
    }
}

fn latest_windows(db: &Connection, now_ms: u64) -> Observed {
    let mut observed = Observed::default();
    let Ok(mut query) = db.prepare("SELECT rowid, ts, account, name, used_pct, resets_at, source FROM usage WHERE provider = 'claude' AND kind = 'window' AND rowid IN (SELECT (SELECT rowid FROM usage WHERE provider = k.provider AND account = k.account AND kind = k.kind AND name = k.name ORDER BY ts DESC, rowid DESC LIMIT 1) FROM (SELECT DISTINCT provider, account, kind, name FROM usage WHERE kind = 'window') k)") else { return observed };
    let rows = query.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?)));
    let seconds = |text: &str| Timestamp::parse(text).ok().map(|at| at.unix_millis().div_euclid(1000));
    for (row, ts, account, name, used, resets_at, source) in rows.into_iter().flatten().flatten() {
        let (Some(window), Some(read_at), Some(used)) = (window_of(&name), seconds(&ts), used) else { continue };
        let resets_at = resets_at.as_deref().and_then(seconds).map(Unix);
        let source = match source.as_deref() {
            Some("header") => Source::Header,
            Some("stream") => Source::Stream,
            Some("web") => Source::Web,
            _ => Source::Cli,
        };
        let account = Label(account);
        if used >= FULL_PCT && resets_at.is_some_and(|at| at.0 as u64 * 1000 > now_ms) {
            observed.limits.push(LimitEvent { account: account.clone(), window, resets_at, at: Unix(read_at) });
        }
        observed.readings.push(Reading { row: RowId(row), account, window, used: Percent(used), resets_at, read_at: Unix(read_at), source });
    }
    observed
}

fn window_of(name: &str) -> Option<Window> {
    match name {
        "session" => Some(Window::FiveHour),
        "weekly_all" => Some(Window::Week),
        scoped => {
            let family = scoped.strip_prefix("weekly_scoped/")?.to_ascii_lowercase();
            [("fable", ModelFamily::Fable), ("opus", ModelFamily::Opus), ("sonnet", ModelFamily::Sonnet), ("haiku", ModelFamily::Haiku)].into_iter().find(|(word, _)| family.contains(word)).map(|(_, family)| Window::Scoped(family))
        }
    }
}

/// Whether a session holds work origin/main lacks: a lane not yet landed.
pub(super) fn holds_unlanded_lane(cwd: &Path) -> bool {
    let tree = Worktree { path: cwd.to_owned(), branch: None };
    Git.lanes(None, &[tree]).iter().any(|lane| lane.unlanded())
}

/// Whether a session holds work origin/main lacks that no queued ticket will land: a lane the lander sent
/// back, or one never enqueued. Such a worker revives before the rest.
pub(super) fn holds_unqueued_lane(session: &str, cwd: &Path) -> bool {
    holds_unlanded_lane(cwd) && !queued(session, cwd)
}

fn queued(session: &str, cwd: &Path) -> bool {
    let cwd = cwd.to_string_lossy();
    let origin = run_within("git", &["-C", &cwd, "remote", "get-url", "--push", "origin"], QUERY).filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(origin) = origin else { return false };
    queue::pending(&queue::directory(&watch::state_home(), &origin)).is_ok_and(|tickets| tickets.iter().any(|(_, ticket)| ticket.session.as_deref() == Some(session)))
}
