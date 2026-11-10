//! What `north fleet waits` reads about a usage limit: the account's reset from the usage table, and
//! which parked session holds a lane that needs landing first.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::fleet::lane_probe::Git;
use crate::fleet::handover::{AccountReadings, Observed};
use crate::fleet::limit::{AccountResets, FULL_PCT};
use crate::fleet::stranded::{Lanes, Worktree};
use crate::foundation::clock::{Timestamp, parse_day_number};
use crate::land::queue;
use crate::land_ticket::{Lane, Phase};
use crate::platform::process::run_within;
use crate::usage::decide::world::{Credit, Unix, Usd};
use crate::usage::{open_ledger, profiles, windows};
use crate::watch;

const QUERY: Duration = Duration::from_secs(10);

/// The reset of the window the account is out of, else of the soonest window, from the latest `usage` rows.
pub(super) struct UsageTable;

impl AccountResets for UsageTable {
    fn reset_ms(&self, account: &str, now_ms: u64) -> Option<u64> {
        if let Some(org) = account.strip_prefix("api:") {
            return credits(now_ms).remove(org).and_then(|credit| credit.next_grant).map(|at| at.0 as u64 * 1000);
        }
        let db = open_ledger(&profiles::home(), None, &|| {}).ok()?;
        window_reset_ms(&db, account, now_ms)
    }
}

fn window_reset_ms(db: &Connection, account: &str, now_ms: u64) -> Option<u64> {
    let read = windows(db, now_ms);
    let ahead = read.readings.iter().filter(|reading| reading.account.0 == account).filter_map(|reading| Some((reading.used.0, u64::try_from(reading.resets_at?.0).ok()? * 1000))).filter(|(_, at)| *at > now_ms);
    let (full, open): (Vec<_>, Vec<_>) = ahead.partition(|(used, _)| *used >= FULL_PCT);
    full.iter().map(|(_, at)| *at).max().or_else(|| open.iter().map(|(_, at)| *at).min())
}

/// The latest reading of every window of every account, with each window that reads full until a reset still
/// ahead also as a limit event, which stays true however old the reading grows.
impl AccountReadings for UsageTable {
    fn observed(&self, now_ms: u64) -> Observed {
        let Ok(db) = open_ledger(&profiles::home(), None, &|| {}) else { return Observed::default() };
        let read = windows(&db, now_ms);
        Observed { credits: credits(now_ms), readings: read.readings, limits: read.limits }
    }
}

fn credits(now_ms: u64) -> BTreeMap<String, Credit> {
    let credits = north_core::credit::standing(&crate::platform::orchestration::path(), &watch::state_home(), Timestamp::from_unix_millis(now_ms as i64));
    credits
        .orgs
        .iter()
        .flat_map(|org| {
            let usd = if org.exhausted { 0.0 } else { org.left };
            let next_grant = org.next_credit.as_deref().and_then(parse_day_number).map(|days| Unix(days * 86_400));
            org.names.iter().map(move |name| (name.clone(), Credit { left: Usd(usd), next_grant }))
        })
        .collect()
}

/// Whether a session holds work origin/main lacks: a land ticket of its own the lander has neither landed nor let go,
/// or the linked worktree it works in (`lane_of`) not yet landed. A primary checkout is never a session's lane, since its
/// HEAD may carry another session's commits (north#801).
pub(super) fn holds_unlanded_lane(session: &str, cwd: &Path, tickets: &BTreeMap<String, Vec<Lane>>) -> bool {
    tickets.get(session).is_some_and(|lanes| lanes.iter().any(|lane| !matches!(lane.phase, Phase::Landed | Phase::Replaced | Phase::Withdrawn))) || own_lane_unlanded(cwd)
}

/// The directory a session's own lane is read from: its cwd, or, when that is a primary checkout, the worktree beside it
/// that the session's recent output names most, the one it made there (north#1007).
pub(super) fn lane_of(cwd: &Path, worktree_named: impl FnOnce(&Path) -> Option<PathBuf>) -> PathBuf {
    if !cwd.join(".git").is_dir() {
        return cwd.to_owned();
    }
    cwd.parent().and_then(|container| worktree_named(&container.join("worktrees"))).unwrap_or_else(|| cwd.to_owned())
}

fn own_lane_unlanded(cwd: &Path) -> bool {
    if !cwd.join(".git").is_file() {
        return false;
    }
    let tree = Worktree { path: cwd.to_owned(), branch: None };
    Git.lanes(None, &[tree]).iter().any(|lane| lane.unlanded())
}

/// Whether a session holds work origin/main lacks that no queued ticket will land: a lane the lander sent
/// back, or one never enqueued. Such a worker revives before the rest.
pub(super) fn holds_unqueued_lane(session: &str, cwd: &Path) -> bool {
    own_lane_unlanded(cwd) && !queued(session, cwd)
}

fn queued(session: &str, cwd: &Path) -> bool {
    let cwd = cwd.to_string_lossy();
    let origin = run_within("git", &["-C", &cwd, "remote", "get-url", "--push", "origin"], QUERY).filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(origin) = origin else { return false };
    queue::pending(&queue::directory(&watch::state_home(), &origin)).is_ok_and(|tickets| tickets.iter().any(|(_, ticket)| ticket.session.as_deref() == Some(session)))
}

#[cfg(test)]
#[path = "limit_tests.rs"]
mod tests;
