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
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};

const WATCH_S: i64 = 10 * 60;
const PRIOR_S: i64 = 60 * 60;
const LEAST: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) launch_failures: usize,
    pub(super) host_exits: usize,
    pub(super) land_refusals: usize,
}

impl Counts {
    fn read(layout: &Layout, start: i64, end: i64) -> Self {
        let (start, end) = (Timestamp::from_unix_seconds(start), Timestamp::from_unix_seconds(end));
        let moves = |machine: &str, from: Option<&str>, to: &str| Store::open(&layout.ledger()).and_then(|store| store.moves_between(machine, from, to, start, end)).unwrap_or(0);
        Self {
            launch_failures: launch_refusals(&threads_ledger(layout), &start.iso(), &end.iso()),
            host_exits: moves("fleet-session", Some("live"), "parked"),
            land_refusals: moves("land-ticket", None, "failed"),
        }
    }
}

/// The kinds that ran faster in the `watched` seconds since the swap than over the hour before it, each counting only once it happened `LEAST` times, so one stray failure rolls nothing back.
pub(super) fn risen(since: Counts, prior: Counts, watched: i64) -> Vec<&'static str> {
    let watched = watched.max(1) as f64;
    let rose = |since: usize, prior: usize| since >= LEAST && since as f64 / watched > prior as f64 / PRIOR_S as f64;
    [("launch failures", since.launch_failures, prior.launch_failures), ("session-host exits", since.host_exits, prior.host_exits), ("land refusals", since.land_refusals, prior.land_refusals)]
        .into_iter()
        .filter(|(_, since, prior)| rose(*since, *prior))
        .map(|(kind, ..)| kind)
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
    let risen = risen(Counts::read(layout, swapped, at), Counts::read(layout, swapped - PRIOR_S, swapped), at - swapped);
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

fn launch_refusals(ledger: &Path, start: &str, end: &str) -> usize {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return 0 };
    db.query_row("SELECT count(*) FROM boundary_causes WHERE source = 'launch' AND cause = 'launch refusal' AND at >= ?1 AND at < ?2", [start, end], |row| row.get::<_, i64>(0))
        .map_or(0, |count| usize::try_from(count).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::{Counts, risen};

    #[test]
    fn a_kind_rolls_the_runtime_back_only_when_it_ran_faster_since_the_swap_than_the_hour_before() {
        let quiet = Counts::default();
        let exits = |host_exits| Counts { host_exits, ..Counts::default() };
        assert_eq!(risen(exits(1), quiet, 300), Vec::<&str>::new(), "one stray exit rolls nothing back");
        assert_eq!(risen(exits(2), quiet, 300), ["session-host exits"]);
        assert_eq!(risen(exits(2), exits(30), 300), Vec::<&str>::new(), "2 in 5 min is slower than 30 an hour");
        assert_eq!(risen(Counts { launch_failures: 3, land_refusals: 3, host_exits: 0 }, Counts { launch_failures: 3, ..quiet }, 600), ["launch failures", "land refusals"]);
    }
}
