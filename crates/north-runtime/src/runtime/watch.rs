use super::failure::DEFERRED;
use super::layout::{self, Layout};
use super::lifecycle::{self, Canarying, Canary};
use super::live::Swap;
use super::pin::{Event, note, now};
use super::update;
use crate::foundation::clock::Timestamp;
use north_offload::land_offload;
use crate::platform::host;
use north_base::lifecycle::Store;
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod exits;

use exits::{capacity_parked, lasting_host_exits};

const WATCH_S: i64 = 10 * 60;
const PRIOR_S: i64 = 60 * 60;
const LEAST: usize = 2;
const SAMPLE: usize = 3;

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

/// What the watch makes of the live runtime's window: nothing, a rollback of a runtime a promotion or an order made live, or one report of a window a rollback opened, which never rolls back again, so one signal cannot walk the runtime back step after step.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Quiet,
    RollBack { why: String, seconds: i64 },
    Report { why: String, swapped: i64 },
}

/// Judges, at `at`, the first `WATCH_S` after the newest swap to `live` in `swaps` on the host `name`: rolls back when launch failures, session-host exits or land refusals rose over the hour before the swap, unless a rollback made that swap.
pub(super) fn judge(layout: &Layout, live: &str, swaps: &[Swap], at: i64, name: &str) -> Verdict {
    let Some(swap) = swaps.iter().rev().find(|swap| swap.runtime == live) else { return Verdict::Quiet };
    let trusted = lifecycle::state_of::<Canarying>(layout, &format!("{name}/{live}")).ok().flatten() == Some(Canary::Trusted);
    if at - swap.at > WATCH_S || trusted {
        return Verdict::Quiet;
    }
    let (since, prior) = (Window::read(layout, swap.at, at, at), Window::read(layout, swap.at - PRIOR_S, swap.at, at));
    let risen = risen(&since, &prior, at - swap.at);
    if risen.is_empty() {
        return Verdict::Quiet;
    }
    let came = if swap.rollback { "came back by rollback" } else { "went live" };
    let why = format!("{} rose in the {}s since {} {came} on {name}, over the hour before", named(&risen, &since, &prior), at - swap.at, layout::short(live));
    match swap.rollback {
        true => Verdict::Report { why, swapped: swap.at },
        false => Verdict::RollBack { why, seconds: at - swap.at },
    }
}

/// For `WATCH_S` after the live runtime's swap, acts on [`judge`]; only `current` moves, so running session hosts keep the runtime they started on.
pub(super) fn watch(layout: &Layout) {
    let Some(live) = layout.live_sha() else { return };
    let swaps: Vec<Swap> = fs::read_to_string(layout.swaps()).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let (at, name) = (now(), host::name());
    let short = layout::short(&live);
    match judge(layout, &live, &swaps, at, &name) {
        Verdict::Quiet => {}
        Verdict::Report { why, swapped } => {
            let window = format!("{live} {swapped}");
            if fs::read_to_string(layout.watch_reported()).is_ok_and(|reported| reported == window) {
                return;
            }
            let _ = fs::write(layout.watch_reported(), &window);
            update::alert_proxy(&format!("north runtime watch: {why}; a rollback's window rolls nothing back, so {short} stays live"));
        }
        Verdict::RollBack { why, seconds } => match update::roll_back(layout, &live) {
            Ok(previous) => {
                note(layout, &Event::RolledBack { sha: live.clone(), host: name, at, seconds, walker: land_offload::settles_fleet_here() });
                update::alert_proxy(&format!("north runtime watch: {why}; rolled back to {} and quarantined {short}", layout::short(&previous)));
            }
            Err(failure) if failure.code == DEFERRED => println!("north runtime watch: {why}; rollback deferred: {}", failure.message),
            Err(failure) => update::alert_proxy(&format!("north runtime watch: {why}; could not roll back: {}", failure.message)),
        },
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
