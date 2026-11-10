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

static READING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d.]*").expect("a reading pattern"));

/// What one span held: the sessions this host started, the reason of each launch the gate refused, and this host's session-host exits and land refusals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Window {
    pub(super) launches: usize,
    pub(super) refusals: Vec<String>,
    pub(super) host_exits: usize,
    pub(super) land_refusals: usize,
}

impl Window {
    fn read(layout: &Layout, start: i64, end: i64) -> Self {
        let (start, end) = (Timestamp::from_unix_seconds(start), Timestamp::from_unix_seconds(end));
        let moves = |machine: &str, from: Option<&str>, to: &str| Store::open(&layout.ledger()).and_then(|store| store.moves_between(machine, from, to, start, end)).unwrap_or(0);
        Self {
            launches: moves("fleet-session", None, "live"),
            refusals: launch_refusals(&threads_ledger(layout), &start.iso(), &end.iso()),
            host_exits: moves("fleet-session", Some("live"), "parked"),
            land_refusals: moves("land-ticket", None, "failed"),
        }
    }
}

/// A refusal's cause: its reason with every reading blanked, so the gate refusing at load 10.19 and at 10.79 is one cause.
fn cause(reason: &str) -> String {
    READING.replace_all(reason, "#").into_owned()
}

/// Whether launches failed more often since the swap than over the hour before: only a refusal whose cause the hour before never saw counts, since the previous runtime shares every other, and only as a share of the launches tried, once the hour before tried `SAMPLE`, so a host that just began launching has no rate to beat.
fn launches_failed_more(since: &Window, prior: &Window) -> bool {
    let shared: HashSet<String> = prior.refusals.iter().map(|reason| cause(reason)).collect();
    let fresh = since.refusals.iter().filter(|reason| !shared.contains(&cause(reason))).count();
    let tried_before = prior.launches + prior.refusals.len();
    fresh >= LEAST && tried_before >= SAMPLE && fresh as f64 / (since.launches + fresh) as f64 > prior.refusals.len() as f64 / tried_before as f64
}

/// The kinds that ran worse in the `watched` seconds since the swap than over the hour before it: launch failures by `launches_failed_more`, and session-host exits and land refusals by rate, each counting only once it happened `LEAST` times, so one stray failure rolls nothing back.
pub(super) fn risen(since: &Window, prior: &Window, watched: i64) -> Vec<&'static str> {
    let watched = watched.max(1) as f64;
    let rose = |since: usize, prior: usize| since >= LEAST && since as f64 / watched > prior as f64 / PRIOR_S as f64;
    [
        ("launch failures", launches_failed_more(since, prior)),
        ("session-host exits", rose(since.host_exits, prior.host_exits)),
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
    let risen = risen(&Window::read(layout, swapped, at), &Window::read(layout, swapped - PRIOR_S, swapped), at - swapped);
    if risen.is_empty() {
        return;
    }
    let short = layout::short(&live);
    let why = format!("{} rose in the {}s since {short} went live on {name}, over the hour before", risen.join(", "), at - swapped);
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

fn launch_refusals(ledger: &Path, start: &str, end: &str) -> Vec<String> {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let Ok(mut statement) = db.prepare("SELECT reason FROM boundary_causes WHERE source = 'launch' AND cause = 'launch refusal' AND at >= ?1 AND at < ?2") else { return Vec::new() };
    statement.query_map([start, end], |row| row.get::<_, String>(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Window, risen};

    fn refused(reasons: &[&str]) -> Vec<String> {
        reasons.iter().map(|reason| (*reason).to_owned()).collect()
    }

    #[test]
    fn a_kind_rolls_the_runtime_back_only_when_it_ran_faster_since_the_swap_than_the_hour_before() {
        let quiet = Window::default();
        let exits = |host_exits| Window { host_exits, ..Window::default() };
        assert_eq!(risen(&exits(1), &quiet, 300), Vec::<&str>::new(), "one stray exit rolls nothing back");
        assert_eq!(risen(&exits(2), &quiet, 300), ["session-host exits"]);
        assert_eq!(risen(&exits(2), &exits(30), 300), Vec::<&str>::new(), "2 in 5 min is slower than 30 an hour");
        let launching = Window { launches: 6, ..Window::default() };
        let broken = Window { launches: 1, refusals: refused(&["session host died at start: exit 101", "session host died at start: exit 101"]), land_refusals: 3, ..Window::default() };
        assert_eq!(risen(&broken, &launching, 600), ["launch failures", "land refusals"]);
    }

    #[test]
    fn launch_refusals_on_a_host_with_no_launches_before_the_swap_or_with_a_cause_the_previous_runtime_shares_roll_nothing_back() {
        let failed = Window { refusals: refused(&["session host died at start: exit 101", "no login for claude:c", "spend tripwire: claude:e rose 31 points in 15 min"]), ..Window::default() };
        assert_eq!(risen(&failed, &Window::default(), 120), Vec::<&str>::new(), "a host that just began launching has no rate to beat");
        let capacity = |load: &str| format!("Capacity leases and unleased heavy load commit {load} of the 8 CPUs the machine can hand out, protected desktop pressure is 0.02% (limit 20%)");
        let before = Window { launches: 6, refusals: vec![capacity("10.19"), capacity("10.19"), capacity("10.19")], ..Window::default() };
        let since = Window { refusals: vec![capacity("10.79"), capacity("10.79")], ..Window::default() };
        assert_eq!(risen(&since, &before, 50), Vec::<&str>::new(), "the hour before refused for the same cause, so the previous runtime shares it (north#1081)");
    }
}
