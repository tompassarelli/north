//! Host saturation: a machine whose capacity-admission queue or root disk has stayed over its limit
//! long enough that launching more work onto it only deepens the pile. Spill: a host that takes no
//! other host's builds, such as the laptop, carrying leased fleet builds while a build host has a free slot.
use std::path::Path;

use serde_json::Value;

use super::alerts::{Action, Node, Record, State, deliver};
use super::host::{Host, Places};
use crate::capacity::admission::default_mib;
use crate::capacity::saturation;

const QUEUED_LIMIT: u64 = saturation::QUEUED_LIMIT as u64;
const HOLD_SECONDS: f64 = 600.0;
const SPILL_HOLD_SECONDS: f64 = 300.0;
const FREE_SLOT_CPUS: f64 = 4.0;
const STATE_FILE: &str = "saturation.json";
const SPILL_FILE: &str = "spill.json";
const PROBE: &str = r#""${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}" capacity host status; echo ---; df -BG --output=avail / | tail -1"#;
const SSH: [&str; 5] = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8"];

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub host: String,
    pub queued: u64,
    pub disk_free_gib: f64,
    pub free_cpus: f64,
    pub fleet_cpus: f64,
}

/// Why a host counts as saturated now, `None` when it has room.
pub fn saturation(reading: &Reading, disk_floor_gib: f64) -> Option<String> {
    let mut causes = Vec::new();
    if reading.queued >= QUEUED_LIMIT {
        causes.push(format!("{} jobs queued in machine-capacity admission (limit {QUEUED_LIMIT})", reading.queued));
    }
    if reading.disk_free_gib < disk_floor_gib {
        causes.push(format!("root disk {:.0} GiB free (floor {disk_floor_gib:.0})", reading.disk_free_gib));
    }
    (!causes.is_empty()).then(|| causes.join("; "))
}

pub fn parse(host: &str, output: &str) -> Option<Reading> {
    let (status, disk) = output.split_once("\n---\n")?;
    let status: Value = serde_json::from_str(status).ok()?;
    let queued = status["queued"].as_array().map_or_else(|| status["queued"].as_u64(), |queued| Some(queued.len() as u64))?;
    let disk_free_gib = disk.trim().trim_end_matches('G').parse().ok()?;
    let free_cpus = status["aggregateCpuLimit"].as_f64().unwrap_or_default() - status["committedBatchCpus"].as_f64().unwrap_or_default();
    let fleet_cpus = status["holding"].as_array().into_iter().flatten().filter(|lease| lease["class"] == "heavy").filter_map(|lease| lease["committedCpus"].as_f64()).sum();
    Some(Reading { host: host.into(), queued, disk_free_gib, free_cpus, fleet_cpus })
}

fn hosts_table(policy: &Path) -> Option<toml_edit::DocumentMut> {
    std::fs::read_to_string(policy).ok()?.parse().ok()
}

/// The `[hosts.NAME]` tables of orchestration.toml that carry an `ssh` destination.
pub fn remote_hosts(policy: &Path) -> Vec<(String, String)> {
    let Some(document) = hosts_table(policy) else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(toml_edit::Item::as_table_like) else { return Vec::new() };
    hosts.iter().filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh")?.as_str()?.to_owned()))).collect()
}

/// The `[hosts.NAME]` tables of orchestration.toml that take other hosts' builds (`offload_builds = true`).
pub fn build_hosts(policy: &Path) -> Vec<String> {
    let Some(document) = hosts_table(policy) else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(toml_edit::Item::as_table_like) else { return Vec::new() };
    hosts.iter().filter(|(_, host)| host.get("offload_builds").and_then(toml_edit::Item::as_bool) == Some(true)).map(|(name, _)| name.to_owned()).collect()
}

/// This machine and every ssh host; a host that does not answer is left out.
pub fn read_hosts(host: &dyn Host, policy: &Path) -> Vec<Reading> {
    let local = host.run(&["hostname"], None, None).map(|name| name.trim().to_owned()).unwrap_or_else(|| "local".into());
    let mut readings: Vec<Reading> = host.run(&["sh", "-c", PROBE], None, None).and_then(|output| parse(&local, &output)).into_iter().collect();
    for (name, destination) in remote_hosts(policy) {
        let mut argv: Vec<&str> = SSH.to_vec();
        argv.extend([destination.as_str(), PROBE]);
        readings.extend(host.run(&argv, None, None).and_then(|output| parse(&name, &output)));
    }
    readings
}

/// Alerts the proxy once per episode when a host has stayed saturated for ten minutes; the record is dropped when it
/// has room again, and a host that did not answer keeps its record.
pub fn saturation_plan(readings: &[Reading], previous: &State, now: f64, disk_floor_gib: f64) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for reading in readings {
        let Some(why) = saturation(reading, disk_floor_gib) else {
            state.remove(&reading.host);
            continue;
        };
        let record = state.entry(reading.host.clone()).or_insert_with(|| Record { since: now, ..Record::default() });
        let held = now - record.since;
        let key = "saturation:fleet:proxy";
        if held >= HOLD_SECONDS && !record.sent.contains_key(key) {
            let text = format!("Host {} saturated for {:.0}m: {why}. Stop launching onto it, move work to a free host, clear the blocker", reading.host, held / 60.0);
            actions.push(Action { repo: reading.host.clone(), key: key.into(), node: "proxy".into(), channel: "fleet", text });
        }
    }
    (state, actions)
}

/// Alerts the proxy once per episode when a host outside `build_hosts` has held leased heavy runs for five minutes
/// while a build host had a free slot; the record is dropped once either stops (north#1036).
pub fn spill_plan(readings: &[Reading], build_hosts: &[String], previous: &State, now: f64) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    let free: Vec<&Reading> = readings.iter().filter(|reading| build_hosts.contains(&reading.host) && reading.free_cpus >= FREE_SLOT_CPUS).collect();
    for reading in readings.iter().filter(|reading| !build_hosts.contains(&reading.host)) {
        if reading.fleet_cpus <= 0.0 || free.is_empty() {
            state.remove(&reading.host);
            continue;
        }
        let record = state.entry(reading.host.clone()).or_insert_with(|| Record { since: now, ..Record::default() });
        let held = now - record.since;
        let key = "spill:fleet:proxy";
        if held >= SPILL_HOLD_SECONDS && !record.sent.contains_key(key) {
            let room = free.iter().map(|host| format!("{} {:.1} free cores", host.host, host.free_cpus)).collect::<Vec<_>>().join(", ");
            let text = format!(
                "Host {} has carried {:.1} cores of leased fleet builds for {:.0}m while build hosts had free slots ({room}). Find the placement that kept them there and move them",
                reading.host,
                reading.fleet_cpus,
                held / 60.0
            );
            actions.push(Action { repo: reading.host.clone(), key: key.into(), node: "proxy".into(), channel: "fleet", text });
        }
    }
    (state, actions)
}

pub fn host_alerts(places: &Places, host: &dyn Host, policy: &Path, now: f64) -> Result<(), String> {
    let readings = read_hosts(host, policy);
    let floor = default_mib("build_disk_free_gib");
    let builders = build_hosts(policy);
    let saturated = deliver(places, host, STATE_FILE, |previous, _: &[Node]| saturation_plan(&readings, previous, now, floor), now);
    let spilled = deliver(places, host, SPILL_FILE, |previous, _: &[Node]| spill_plan(&readings, &builders, previous, now), now);
    saturated.and(spilled)
}

#[cfg(test)]
#[path = "capacity_tests.rs"]
mod tests;
