//! The held units no registry holds: a placing host that forgot or ended a session leaves its unit on this
//! machine idle until the unit's own 12-hour detach limit, holding a resident slot and memory. On 2026-11-07
//! four such units (north-643-d2035, north-671-d0221, north-703-d0221, north-706-d0216) ran 6 to 10 hours on
//! gex45 until they were stopped by hand.

use std::collections::BTreeSet;
use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use north_session_host::held::{HeldUnit, session_of};

use super::model::Status;
use super::registry::Fleet;
use super::store::SessionRow;

/// How long a unit runs before a missing row can mean its session was forgotten rather than not yet published.
pub const GRACE: Duration = Duration::from_secs(30 * 60);
/// How recently the fleet table and a row in it must have been written for their word to count.
pub const FRESH: Duration = Duration::from_secs(10 * 60);

/// The fleet table as this host's last tick cached it, and when that tick wrote it.
pub struct Table<'a> {
    pub rows: &'a [SessionRow],
    pub cached_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stray {
    pub unit: String,
    pub why: String,
}

/// The units in `units` to stop: past `GRACE`, no link attached, no live record here, and in a fresh table either
/// no row names the session or every row says it exited. A stale row keeps its unit, since the host that owns the
/// row may only be asleep, and a stale table keeps every unit.
pub fn strays(units: &[HeldUnit], live_here: &BTreeSet<String>, table: &Table, now_ms: u64) -> Vec<Stray> {
    let fresh = |at_ms: u64| now_ms.saturating_sub(at_ms) < millis(FRESH);
    if !fresh(table.cached_ms) {
        return Vec::new();
    }
    units
        .iter()
        .filter(|held| !held.attached && now_ms.saturating_sub(held.active_since_ms) >= millis(GRACE))
        .filter_map(|held| {
            let session = session_of(&held.unit)?;
            if live_here.contains(session) {
                return None;
            }
            let rows: Vec<&SessionRow> = table.rows.iter().filter(|row| row.name == session).collect();
            if rows.iter().any(|row| row.status != "exited" || !fresh(updated_ms(row))) {
                return None;
            }
            let hours = now_ms.saturating_sub(held.active_since_ms) / 3_600_000;
            let why = match rows.first() {
                None => format!("no registry holds {session}, and no link has attached to its unit, active {hours} h"),
                Some(row) => format!("{}'s registry records {session} exited, and no link has attached to its unit, active {hours} h", row.host),
            };
            Some(Stray { unit: held.unit.clone(), why })
        })
        .collect()
}

/// Stops each held unit on this machine that `strays` names, one line per unit stopped or not.
pub fn stop_strays(fleet: &Fleet, now_ms: u64) -> Vec<String> {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else { return Vec::new() };
    let units = north_session_host::held::active_units(&std::path::Path::new(&home).join(".local/state/north/held"));
    if units.is_empty() {
        return Vec::new();
    }
    let (Some(cached_ms), Ok(Some(rows)), Ok(records)) = (fleet.sessions_cached_ms(), fleet.cached_sessions(), fleet.sessions()) else { return Vec::new() };
    let live_here: BTreeSet<String> = records.iter().filter(|record| fleet.status(record) != Status::Exited).map(|record| record.name.clone()).collect();
    strays(&units, &live_here, &Table { rows: &rows, cached_ms }, now_ms)
        .into_iter()
        .map(|stray| match north_session_host::held::stop(&stray.unit) {
            Ok(()) => format!("stopped stray unit {}: {}", stray.unit, stray.why),
            Err(error) => format!("could not stop stray unit {}: {error}; {}", stray.unit, stray.why),
        })
        .collect()
}

fn updated_ms(row: &SessionRow) -> u64 {
    Timestamp::parse(&row.updated).ok().and_then(|at| u64::try_from(at.unix_millis()).ok()).unwrap_or(0)
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "strays_tests.rs"]
mod tests;
