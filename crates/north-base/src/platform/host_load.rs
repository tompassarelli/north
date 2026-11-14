//! The load each configured host reports, read by one probe the monitor and the fleet board both draw from.

use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item};

use super::process;

const PROBE: &str = "echo \"runtime $(readlink \"${XDG_STATE_HOME:-$HOME/.local/state}/north/current\")\"; \
echo \"load $(cut -d' ' -f1 /proc/loadavg)\"; echo \"cores $(nproc)\"; \
echo \"follow $(systemctl --user is-active north-runtime-follow.timer 2>/dev/null)\"; \
echo \"stable $(cut -c1-10 \"${XDG_STATE_HOME:-$HOME/.local/state}/north/channel/stable\" 2>/dev/null)\"";
const SSH_LIMIT: Duration = Duration::from_secs(12);
const LOCAL_LIMIT: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostState {
    pub runtime: Option<String>,
    pub follow: Option<String>,
    pub stable: Option<String>,
    pub load1: Option<f64>,
    pub cores: Option<u32>,
}

impl HostState {
    pub fn load_per_core(&self) -> Option<f64> {
        Some(self.load1? / f64::from(self.cores?.max(1)))
    }
}

pub struct HostTarget {
    pub name: String,
    pub ssh: Option<String>,
}

pub struct HostReading {
    pub name: String,
    pub state: Result<HostState, String>,
}

pub fn short(sha: &str) -> String {
    sha.rsplit('/').next().unwrap_or(sha).chars().take(7).collect()
}

/// This machine, named `local`, then each host of the orchestration policy that names an ssh destination.
pub fn targets(orchestration: &DocumentMut, local: &str) -> Vec<HostTarget> {
    let configured = orchestration.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter()).filter_map(|(name, host)| Some(HostTarget { name: name.to_owned(), ssh: Some(host.get("ssh")?.as_str()?.to_owned()) }));
    std::iter::once(HostTarget { name: local.to_owned(), ssh: None }).chain(configured).collect()
}

/// Every target's load, each probed on its own thread, so a host that does not answer costs its own timeout and no other host's.
pub fn read_all(targets: &[HostTarget]) -> Vec<HostReading> {
    std::thread::scope(|scope| {
        let probes: Vec<_> = targets.iter().map(|target| scope.spawn(move || HostReading { name: target.name.clone(), state: read(target) })).collect();
        probes.into_iter().zip(targets).map(|(probe, target)| probe.join().unwrap_or_else(|_| HostReading { name: target.name.clone(), state: Err(format!("{}: probe panicked", target.name)) })).collect()
    })
}

pub fn read(host: &HostTarget) -> Result<HostState, String> {
    let (command, limit) = match &host.ssh {
        Some(destination) => {
            let mut command = Command::new("ssh");
            command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, PROBE]);
            (command, SSH_LIMIT)
        }
        None => {
            let mut command = Command::new("sh");
            command.args(["-c", PROBE]);
            (command, LOCAL_LIMIT)
        }
    };
    let text = match process::try_run_command_traced_within(command, None, limit) {
        Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Ok(Err(why)) => return Err(format!("{}: {why}", host.name)),
        Err(error) => return Err(format!("{}: {error}", host.name)),
    };
    let state = parse(&text);
    if state.runtime.is_none() && state.load1.is_none() {
        return Err(format!("{}: probe printed nothing", host.name));
    }
    Ok(state)
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
            "stable" if !value.is_empty() => state.stable = Some(value.to_owned()),
            _ => {}
        }
    }
    state
}
