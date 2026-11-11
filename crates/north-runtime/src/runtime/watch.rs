use super::failure::DEFERRED;
use super::layout::{self, Layout};
use super::lifecycle::{self, Canarying, Canary};
use super::live::Swap;
use super::pin::{Event, note, now};
use super::update;
use crate::foundation::clock::Timestamp;
use crate::land_offload;
use crate::platform::host;
use north_base::lifecycle::Store;
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const WATCH_S: i64 = 10 * 60;
const PRIOR_S: i64 = 60 * 60;
const LEAST: usize = 2;
const SAMPLE: usize = 3;
const RESTART_S: i64 = 60;
const SCRATCH: &str = "vn-";

static READING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d.]*").expect("a reading pattern"));

/// What one span held: the sessions this host started, the reason of each launch that failed to start after placement let it through, and this host's session-host exits and land refusals; a launch placement refused or queued is a decision, not a failure, so no span holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Window {
    pub(super) launches: usize,
    pub(super) failures: Vec<String>,
    pub(super) host_exits: Vec<String>,
    pub(super) land_refusals: usize,
}

impl Window {
    fn read(layout: &Layout, start: i64, end: i64, judged: i64) -> Self {
        let (start, end) = (Timestamp::from_unix_seconds(start), Timestamp::from_unix_seconds(end));
        let moves = |machine: &str, from: Option<&str>, to: &str| Store::open(&layout.ledger()).and_then(|store| store.moves_between(machine, from, to, start, end)).unwrap_or(0);
        Self {
            launches: moves("fleet-session", None, "live"),
            failures: launch_failures(&threads_ledger(layout), &start.iso(), &end.iso()),
            host_exits: lasting_host_exits(&layout.ledger(), (start, end), Timestamp::from_unix_seconds(judged), &capacity_parked(&threads_ledger(layout), &start.iso(), &end.iso())),
            land_refusals: moves("land-ticket", None, "failed"),
        }
    }
}

/// The sessions whose host stopped in `span` and went on running nothing: a host the supervisor started again within `RESTART_S` is a restart, as a promotion moving hosts onto the new runtime makes, and a session the fleet stopped within `RESTART_S` (`north fleet stop`, forget, retire, or a launch whose first turn the account limit refused) is stopped by order, and a `vn-*` session is the canary walk's own scratch host, so none is an exit, and a stop younger than `RESTART_S` at `judged` is not yet known to be any of them; a session in `parked`, which the fleet parked for capacity such as its account reaching its limit, is a decision rather than an exit.
fn lasting_host_exits(ledger: &Path, (start, end): (Timestamp, Timestamp), judged: Timestamp, parked: &HashSet<String>) -> Vec<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let Ok(mut statement) = db.prepare(
        "SELECT stop.instance FROM lifecycle_moves stop WHERE stop.machine = 'fleet-session' AND stop.from_state = 'live' AND stop.to_state = 'parked' AND stop.refused = 0 \
         AND stop.at_ms >= ?1 AND stop.at_ms < ?2 AND stop.at_ms <= ?3 - ?4 AND substr(stop.instance, 1, length(?5)) <> ?5 \
         AND NOT EXISTS (SELECT 1 FROM lifecycle_moves again WHERE again.machine = stop.machine AND again.instance = stop.instance AND again.to_state IN ('live', 'stopped') AND again.refused = 0 AND again.move_id > stop.move_id AND again.at_ms <= stop.at_ms + ?4) ORDER BY stop.instance",
    ) else {
        return Vec::new();
    };
    let stopped = statement.query_map(rusqlite::params![start.unix_millis(), end.unix_millis(), judged.unix_millis(), RESTART_S * 1000, SCRATCH], |row| row.get::<_, String>(0));
    let sessions = stopped.map(|rows| rows.filter_map(Result::ok).map(|instance| instance.split_once('@').map_or(instance.clone(), |(name, _)| name.to_owned())).collect::<Vec<_>>()).unwrap_or_default();
    sessions.into_iter().filter(|name| !parked.contains(name)).collect()
}

/// The sessions the fleet parked for capacity in the span, by the boundary cause it recorded for each.
fn capacity_parked(ledger: &Path, start: &str, end: &str) -> HashSet<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return HashSet::new() };
    let Ok(mut statement) = db.prepare("SELECT session FROM boundary_causes WHERE source = 'fleet' AND cause = 'capacity' AND at >= ?1 AND at < ?2") else { return HashSet::new() };
    statement.query_map([start, end], |row| row.get::<_, String>(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

/// A failure's cause: its reason with every reading blanked, so a failure at load 10.19 and at 10.79 is one cause.
fn cause(reason: &str) -> String {
    READING.replace_all(reason, "#").into_owned()
}

/// The failures since the swap whose cause the hour before never saw, since the previous runtime shares every other.
fn fresh_failures<'a>(since: &'a Window, prior: &Window) -> Vec<&'a str> {
    let shared: HashSet<String> = prior.failures.iter().map(|reason| cause(reason)).collect();
    since.failures.iter().filter(|reason| !shared.contains(&cause(reason))).map(String::as_str).collect()
}

/// Whether launches failed more often since the swap than over the hour before: only a fresh failure counts, and only as a share of the launches tried, once the hour before tried `SAMPLE`, so a host that just began launching has no rate to beat.
fn launches_failed_more(since: &Window, prior: &Window) -> bool {
    let fresh = fresh_failures(since, prior).len();
    let tried_before = prior.launches + prior.failures.len();
    fresh >= LEAST && tried_before >= SAMPLE && fresh as f64 / (since.launches + fresh) as f64 > prior.failures.len() as f64 / tried_before as f64
}

/// What a rollback names: each kind that rose, and for launch failures the distinct fresh causes it counted, and for session-host exits the sessions whose host it counted.
fn named(risen: &[&str], since: &Window, prior: &Window) -> String {
    let mut causes: Vec<String> = fresh_failures(since, prior).into_iter().map(cause).collect();
    causes.sort();
    causes.dedup();
    let describe = |kind: &str| match kind {
        "launch failures" => format!("launch failures ({})", causes.join("; ")),
        "session-host exits" => format!("session-host exits ({})", since.host_exits.join(", ")),
        other => other.to_owned(),
    };
    risen.iter().map(|kind| describe(kind)).collect::<Vec<_>>().join(", ")
}

/// The kinds that ran worse in the `watched` seconds since the swap than over the hour before it: launch failures by `launches_failed_more`, and session-host exits and land refusals by rate, each counting only once it happened `LEAST` times, so one stray failure rolls nothing back.
pub(super) fn risen(since: &Window, prior: &Window, watched: i64) -> Vec<&'static str> {
    let watched = watched.max(1) as f64;
    let rose = |since: usize, prior: usize| since >= LEAST && since as f64 / watched > prior as f64 / PRIOR_S as f64;
    [
        ("launch failures", launches_failed_more(since, prior)),
        ("session-host exits", rose(since.host_exits.len(), prior.host_exits.len())),
        ("land refusals", rose(since.land_refusals, prior.land_refusals)),
    ]
    .into_iter()
    .filter(|(_, worse)| *worse)
    .map(|(kind, _)| kind)
    .collect()
}

/// For `WATCH_S` after the live runtime's swap, rolls it back when launch failures, session-host exits or land refusals rose over the hour before the swap; only `current` moves, so running session hosts keep the runtime they started on.
pub(super) fn watch(layout: &Layout) {
    let Some(live) = layout.live_sha() else { return };
    let swaps: Vec<Swap> = fs::read_to_string(layout.swaps()).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let Some(swapped) = swaps.iter().rev().find(|swap| swap.runtime == live).map(|swap| swap.at) else { return };
    let at = now();
    let name = host::name();
    let trusted = lifecycle::state_of::<Canarying>(layout, &format!("{name}/{live}")).ok().flatten() == Some(Canary::Trusted);
    if at - swapped > WATCH_S || trusted {
        return;
    }
    let (since, prior) = (Window::read(layout, swapped, at, at), Window::read(layout, swapped - PRIOR_S, swapped, at));
    let risen = risen(&since, &prior, at - swapped);
    if risen.is_empty() {
        return;
    }
    let short = layout::short(&live);
    let why = format!("{} rose in the {}s since {short} went live on {name}, over the hour before", named(&risen, &since, &prior), at - swapped);
    match update::roll_back(layout, &live) {
        Ok(previous) => {
            note(layout, &Event::RolledBack { sha: live.clone(), host: name, at, seconds: at - swapped, walker: land_offload::settles_fleet_here() });
            update::alert_proxy(&format!("north runtime watch: {why}; rolled back to {} and quarantined {short}", layout::short(&previous)));
        }
        Err(failure) if failure.code == DEFERRED => println!("north runtime watch: {why}; rollback deferred: {}", failure.message),
        Err(failure) => update::alert_proxy(&format!("north runtime watch: {why}; could not roll back: {}", failure.message)),
    }
}

fn threads_ledger(layout: &Layout) -> PathBuf {
    let state = layout.state.parent().unwrap_or(&layout.state).join("threads");
    let replica = layout::home().join(".config/threads/remote").exists();
    state.join(if replica { "snapshot.db" } else { "threads.db" })
}

fn launch_failures(ledger: &Path, start: &str, end: &str) -> Vec<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let Ok(mut statement) = db.prepare("SELECT reason FROM boundary_causes WHERE source = 'launch' AND cause = 'launch failure' AND at >= ?1 AND at < ?2") else { return Vec::new() };
    statement.query_map([start, end], |row| row.get::<_, String>(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
