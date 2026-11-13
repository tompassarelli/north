use crate::foundation::clock::Timestamp;
use north_base::lifecycle::ORPHANED_LIKE;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::path::Path;

pub(super) const RESTART_S: i64 = 60;
const SCRATCH: [&str; 2] = ["vn-", "canary-"];

/// The sessions whose host died mid-work in `span`: it stopped with nobody asking and went on running nothing, which only the store's own move for a holder that is gone records, while a turn it started at or after the span's start had no completed run end after it. A session its host parked at its idle limit, as every idle session is after `--idle-exit`, is asked for; a host the supervisor started again within `RESTART_S` is a restart, as a promotion moving hosts onto the new runtime makes; a session the fleet stopped within `RESTART_S` (`north fleet stop`, forget, retire, or a launch whose first turn the account limit refused) is stopped by order; a `vn-*` or `canary-*` session is the verify walk's or the canary's own probe; a host that finished its turns, or started none since the span began, died idle; so none is an exit, and a stop younger than `RESTART_S` at `judged` is not yet known to be any of them. A session in `parked`, which the fleet parked for capacity such as its account reaching its limit, is a decision rather than an exit.
pub(super) fn lasting_host_exits(ledger: &Path, (start, end): (Timestamp, Timestamp), judged: Timestamp, parked: &HashSet<String>) -> Vec<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let Ok(mut statement) = db.prepare(
        "SELECT stop.instance, stop.at_ms FROM lifecycle_moves stop WHERE stop.machine = 'fleet-session' AND stop.from_state = 'live' AND stop.to_state = 'parked' AND stop.refused = 0 AND stop.cause LIKE ?5 \
         AND stop.at_ms >= ?1 AND stop.at_ms < ?2 AND stop.at_ms <= ?3 - ?4 \
         AND NOT EXISTS (SELECT 1 FROM lifecycle_moves again WHERE again.machine = stop.machine AND again.instance = stop.instance AND again.to_state IN ('live', 'stopped') AND again.refused = 0 AND again.move_id > stop.move_id AND again.at_ms <= stop.at_ms + ?4) ORDER BY stop.instance",
    ) else {
        return Vec::new();
    };
    let stopped = statement.query_map(rusqlite::params![start.unix_millis(), end.unix_millis(), judged.unix_millis(), RESTART_S * 1000, ORPHANED_LIKE], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)));
    let stops: Vec<(String, i64)> = stopped.map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default();
    stops
        .into_iter()
        .filter(|(instance, stopped_ms)| !SCRATCH.iter().any(|prefix| instance.starts_with(prefix)) && died_mid_work(&db, instance, start.unix_millis(), *stopped_ms))
        .map(|(instance, _)| session_of(&instance).to_owned())
        .filter(|name| !parked.contains(name))
        .collect()
}

fn session_of(instance: &str) -> &str {
    instance.split_once('@').map_or(instance, |(name, _)| name)
}

/// Whether the host `instance` started a turn between `since_ms` and its stop at `stopped_ms` that no completed run finished: the turn its launch opened, or the newest message row it took, since a host a promotion moved onto the new runtime in place keeps the launch it began with.
fn died_mid_work(db: &Connection, instance: &str, since_ms: i64, stopped_ms: i64) -> bool {
    let name = session_of(instance);
    let newest = |sql: &str, key: &str| db.query_row(sql, rusqlite::params![key, since_ms, stopped_ms], |row| row.get::<_, Option<i64>>(0)).ok().flatten();
    let launched = newest(
        "SELECT max(at_ms) FROM lifecycle_moves WHERE machine = 'fleet-session' AND instance = ?1 AND from_state IS NULL AND to_state = 'live' AND refused = 0 AND at_ms >= ?2 AND at_ms <= ?3",
        instance,
    );
    let taken = newest(
        "SELECT max(at_ms) FROM lifecycle_moves WHERE machine = 'row-delivery' AND substr(instance, 1, length(?1) + 1) = ?1 || '/' AND to_state = 'delivered' AND refused = 0 AND at_ms >= ?2 AND at_ms <= ?3",
        name,
    );
    let Some(turn_ms) = launched.max(taken) else { return false };
    let finished = db.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE session = ?1 AND exit = 'completed' AND ended_ms >= ?2)", rusqlite::params![name, turn_ms], |row| row.get::<_, bool>(0));
    !finished.unwrap_or(false)
}

/// The sessions the fleet parked for capacity in the span, by the boundary cause it recorded for each.
pub(super) fn capacity_parked(ledger: &Path, start: &str, end: &str) -> HashSet<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return HashSet::new() };
    let Ok(mut statement) = db.prepare("SELECT session FROM boundary_causes WHERE source = 'fleet' AND cause = 'capacity' AND at >= ?1 AND at < ?2") else { return HashSet::new() };
    statement.query_map([start, end], |row| row.get::<_, String>(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}
