use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The coordinator's floor of live remote runners across the grid; a guess: it lets one cpx host close for an eval (16 of 28 left) and is four times the 4 that starved landings on 2026-11-14; the line wait measured at the next eval closure settles it, and it rises if that wait passes 120 s at 16 runners.
pub const RUNNER_FLOOR: usize = 16;
pub const IDLE_CORES: f64 = 0.1;
pub const IDLE_MS: i64 = 5 * 60 * 1000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HostReading {
    pub host: String,
    pub runners: Option<usize>,
    #[serde(default)]
    pub land_checks: Vec<LeaseUse>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LeaseUse {
    pub lease: String,
    pub cores: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    runners_low: bool,
    #[serde(default)]
    idle_since: BTreeMap<String, BTreeMap<String, i64>>,
    #[serde(default)]
    idle_paged: BTreeSet<String>,
}

pub fn step(mut state: State, at_ms: i64, hosts: &[HostReading]) -> (State, Vec<String>) {
    let mut pages = Vec::new();
    let live: usize = hosts.iter().filter_map(|host| host.runners).sum();
    let low = live < RUNNER_FLOOR;
    if low && !state.runners_low {
        let each: Vec<String> = hosts.iter().map(|host| format!("{} {}", host.host, host.runners.map_or_else(|| "unread".to_owned(), |count| count.to_string()))).collect();
        pages.push(format!("capacity: {live} live remote runners across the grid, under the coordinator's floor of {RUNNER_FLOOR} ({}); hand-ins wait on the runners left", each.join(", ")));
    }
    state.runners_low = low;
    let mut idle_since = BTreeMap::new();
    let mut idle_paged = BTreeSet::new();
    for host in hosts {
        let earlier = state.idle_since.get(&host.host);
        let since: BTreeMap<String, i64> = host
            .land_checks
            .iter()
            .filter(|lease| lease.cores.is_some_and(|cores| cores < IDLE_CORES))
            .map(|lease| (lease.lease.clone(), earlier.and_then(|leases| leases.get(&lease.lease)).copied().unwrap_or(at_ms)))
            .collect();
        let idle = since.values().filter(|start| at_ms - **start >= IDLE_MS).count();
        if idle * 2 > host.land_checks.len() {
            if !state.idle_paged.contains(&host.host) {
                pages.push(format!("capacity: {idle} of {} land-check leases on {} used under {IDLE_CORES} cores each for {} minutes; their checks are waiting, not working", host.land_checks.len(), host.host, IDLE_MS / 60_000));
            }
            idle_paged.insert(host.host.clone());
        }
        if !since.is_empty() {
            idle_since.insert(host.host.clone(), since);
        }
    }
    state.idle_since = idle_since;
    state.idle_paged = idle_paged;
    (state, pages)
}
