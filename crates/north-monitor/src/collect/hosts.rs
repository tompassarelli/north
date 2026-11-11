use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{HostTarget, run};
use crate::snapshot::HostState;

const PROBE: &str = "echo \"runtime $(readlink \"${XDG_STATE_HOME:-$HOME/.local/state}/north/current\")\"; \
echo \"load $(cut -d' ' -f1 /proc/loadavg)\"; echo \"cores $(nproc)\"; \
echo \"follow $(systemctl --user is-active north-runtime-follow.timer 2>/dev/null)\"";

pub fn short(sha: &str) -> String {
    sha.rsplit('/').next().unwrap_or(sha).chars().take(7).collect()
}

pub fn host_name(name: &str) -> &str {
    name.rsplit('@').next().unwrap_or(name)
}

fn parse(text: &str) -> HostState {
    let mut state = HostState::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else { continue };
        let value = value.trim();
        match key {
            "runtime" if !value.is_empty() => state.runtime = Some(short(value)),
            "load" => state.load1 = value.parse().ok(),
            "cores" => state.cores = value.parse().ok(),
            "follow" if !value.is_empty() => state.follow = Some(value.to_owned()),
            _ => {}
        }
    }
    state
}

pub fn read(host: &HostTarget) -> Result<HostState, String> {
    let text = match &host.ssh {
        Some(destination) => run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, PROBE], Duration::from_secs(12))?,
        None => run("sh", &["-c", PROBE], Duration::from_secs(4))?,
    };
    let state = parse(&text);
    if state.runtime.is_none() && state.load1.is_none() {
        return Err(format!("{}: probe printed nothing", host.name));
    }
    Ok(state)
}

#[derive(Deserialize)]
struct Process {
    host: String,
    #[serde(default)]
    runtime: Option<String>,
}

pub fn lag() -> Result<BTreeMap<String, BTreeMap<String, u32>>, String> {
    let text = run("north", &["runtime", "lag", "--json"], Duration::from_secs(20))?;
    let processes: Vec<Process> = serde_json::from_str(&text).map_err(|error| format!("runtime lag --json: {error}"))?;
    let mut hosts: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for process in processes {
        let runtime = process.runtime.as_deref().filter(|sha| !sha.is_empty()).map_or_else(|| "unknown".to_owned(), short);
        *hosts.entry(host_name(&process.host).to_owned()).or_default().entry(runtime).or_default() += 1;
    }
    Ok(hosts)
}

#[derive(Deserialize)]
struct Live {
    sha: String,
    #[serde(rename = "liveAt")]
    live_at: u64,
    #[serde(default)]
    placed: Vec<String>,
}

pub fn runtimes_as_of(state: &Path, at: u64) -> Result<BTreeMap<String, String>, String> {
    let path = state.join("runtime-live.jsonl");
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hosts = BTreeMap::new();
    for live in text.lines().filter_map(|line| serde_json::from_str::<Live>(line).ok()).filter(|live| live.live_at <= at) {
        for host in &live.placed {
            hosts.insert(host_name(host).to_owned(), short(&live.sha));
        }
    }
    Ok(hosts)
}
