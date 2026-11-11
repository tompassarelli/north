use std::path::Path;

use crate::collect::{default_hosts, fleet, health, hosts, landing, rollout};
use crate::snapshot::{Host, HostState, Reading, Snapshot, Trends};

const NOT_KEPT: &str = "not kept in history";

pub fn snapshots(state: &Path, from: u64, to: u64, step: u64) -> Result<Vec<Snapshot>, String> {
    let runs = fleet::runs(state, from.saturating_sub(1800), to)?;
    let machines = fleet::machines();
    let spans = health::builds_all(state).unwrap_or_default();
    let queue = state.join("land-queue");
    let mut cache = landing::Cache::default();
    let mut trends = Trends::default();
    let mut out = Vec::new();
    let ladder = if state.join("ladder-level").exists() { None } else { Some(0) };
    let mut at = from;
    while at <= to {
        let fleet = fleet::as_of(&runs, &machines, at);
        trends.sample(at, Some(fleet.busy()), None);
        let runtimes = hosts::runtimes_as_of(state, at)?;
        let hosts = default_hosts()
            .into_iter()
            .map(|target| {
                let state = match runtimes.get(&target.name) {
                    Some(runtime) => Reading::ok(at, HostState { runtime: Some(runtime.clone()), ..HostState::default() }),
                    None => Reading::failed(at, "no runtime-live record for this host"),
                };
                Host { name: target.name, state }
            })
            .collect();
        out.push(Snapshot {
            at,
            fleet: Reading::ok(at, fleet),
            landing: Reading::ok(at, landing::read(&queue, at, &mut cache)?),
            landers: Reading::failed(at, NOT_KEPT),
            hosts,
            lag: Reading::failed(at, NOT_KEPT),
            rollout: Reading::ok(at, rollout::read(state, at)?),
            health: Reading::failed(at, NOT_KEPT),
            builds: Reading::ok(at, health::builds_of(&spans, at as f64)),
            ladder: ladder.map_or_else(|| Reading::failed(at, NOT_KEPT), |level| Reading::ok(at, level)),
            holds: Reading::failed(at, NOT_KEPT),
            trends: trends.clone(),
        });
        at += step.max(1);
    }
    Ok(out)
}
