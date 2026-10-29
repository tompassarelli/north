//! Host saturation: a machine whose capacity-admission queue or root disk has stayed over its limit
//! long enough that launching more work onto it only deepens the pile.
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::alerts::{Action, Node, Record, State, deliver};
use super::host::{Host, Places};
use crate::capacity::admission::default_mib;

const QUEUED_LIMIT: u64 = 4;
const HOLD_SECONDS: f64 = 600.0;
const STATE_FILE: &str = "saturation.json";
const PROBE: &str = r#"bun "$(dirname "$(agents path machine-capacity)")/scripts/machine-capacity.mjs" status; echo ---; df -BG --output=avail / | tail -1"#;
const SSH: [&str; 5] = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8"];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    pub(crate) host: String,
    pub(crate) queued: u64,
    pub(crate) disk_free_gib: f64,
}

/// Why a host counts as saturated now, `None` when it has room.
pub(crate) fn saturation(reading: &Reading, disk_floor_gib: f64) -> Option<String> {
    let mut causes = Vec::new();
    if reading.queued >= QUEUED_LIMIT {
        causes.push(format!("{} jobs queued in machine-capacity admission (limit {QUEUED_LIMIT})", reading.queued));
    }
    if reading.disk_free_gib < disk_floor_gib {
        causes.push(format!("root disk {:.0} GiB free (floor {disk_floor_gib:.0})", reading.disk_free_gib));
    }
    (!causes.is_empty()).then(|| causes.join("; "))
}

pub(crate) fn parse(host: &str, output: &str) -> Option<Reading> {
    let (status, disk) = output.split_once("\n---\n")?;
    let status: Value = serde_json::from_str(status).ok()?;
    let queued = status["queued"].as_array().map_or_else(|| status["queued"].as_u64(), |queued| Some(queued.len() as u64))?;
    let disk_free_gib = disk.trim().trim_end_matches('G').parse().ok()?;
    Some(Reading { host: host.into(), queued, disk_free_gib })
}

/// The `[hosts.NAME]` tables of orchestration.toml that carry an `ssh` destination.
pub(crate) fn remote_hosts(policy: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(policy) else { return Vec::new() };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(toml_edit::Item::as_table_like) else { return Vec::new() };
    hosts.iter().filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh")?.as_str()?.to_owned()))).collect()
}

/// This machine and every ssh host; a host that does not answer is left out.
pub(crate) fn read_hosts(host: &dyn Host, policy: &Path) -> Vec<Reading> {
    let local = host.run(&["hostname"], None, None).map(|name| name.trim().to_owned()).unwrap_or_else(|| "local".into());
    let mut readings: Vec<Reading> = host.run(&["sh", "-c", PROBE], None, None).and_then(|output| parse(&local, &output)).into_iter().collect();
    for (name, destination) in remote_hosts(policy) {
        let mut argv: Vec<&str> = SSH.to_vec();
        argv.extend([destination.as_str(), PROBE]);
        readings.extend(host.run(&argv, None, None).and_then(|output| parse(&name, &output)));
    }
    readings
}

/// Alerts the proxy once a host has stayed saturated for ten minutes; the record is dropped when it
/// has room again, and a host that did not answer keeps its record.
pub(crate) fn saturation_plan(readings: &[Reading], previous: &State, now: f64, disk_floor_gib: f64) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for reading in readings {
        let Some(why) = saturation(reading, disk_floor_gib) else {
            state.remove(&reading.host);
            continue;
        };
        let record = state.entry(reading.host.clone()).or_insert_with(|| Record { since: now, sent: BTreeMap::new() });
        let held = now - record.since;
        let key = "saturation:fleet:proxy";
        if held >= HOLD_SECONDS && record.due(key, now) {
            let text = format!("Host {} saturated for {:.0}m: {why}. Stop launching onto it, move work to a free host, clear the blocker", reading.host, held / 60.0);
            actions.push(Action { repo: reading.host.clone(), key: key.into(), node: "proxy".into(), channel: "fleet", text });
        }
    }
    (state, actions)
}

pub(crate) fn saturation_alert(places: &Places, host: &dyn Host, policy: &Path, now: f64) -> Result<(), String> {
    let readings = read_hosts(host, policy);
    let floor = default_mib("build_disk_free_gib");
    deliver(places, host, STATE_FILE, |previous, _: &[Node]| saturation_plan(&readings, previous, now, floor), now)
}
