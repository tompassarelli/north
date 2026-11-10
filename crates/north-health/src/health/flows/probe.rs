//! Reads every host's flow facts (this one in process, the rest over ssh) and the ledger snapshot into one `World`.
use std::path::Path;
use std::thread;

use super::{Host, HostFacts, Snapshot, World, facts};

pub const REMOTE_FACTS: &str = r#""$HOME/.local/state/north/current/north" health facts"#;

/// Each `[hosts.NAME]` in orchestration.toml that this host reaches over ssh, by its `ssh` or else its `deploy_ssh`.
fn remote_hosts() -> Vec<(String, String)> {
    let Ok(document) = crate::platform::orchestration::read() else { return Vec::new() };
    let here = crate::platform::host::name();
    let Some(hosts) = document.get("hosts").and_then(toml_edit::Item::as_table_like) else { return Vec::new() };
    hosts
        .iter()
        .filter(|(name, _)| *name != here)
        .filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh").or_else(|| host.get("deploy_ssh"))?.as_str()?.to_owned())))
        .collect()
}

fn remote(destination: &str) -> Result<HostFacts, String> {
    let output = super::super::command("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", destination, REMOTE_FACTS], None, 25)?;
    serde_json::from_str(&output).map_err(|error| format!("north health facts printed no facts: {error}"))
}

/// When the oldest origin/main commit missing from `runtime` landed, `None` when `runtime` is origin/main.
fn behind_since(repo: &Path, runtime: Option<&str>) -> Result<Option<f64>, String> {
    let runtime = runtime.ok_or("no current runtime link")?;
    let range = format!("{runtime}..origin/main");
    let times = super::super::command("git", &["log", "--first-parent", "--reverse", "--format=%ct", &range], Some(repo), 10).map_err(|why| format!("runtime {runtime} is not on origin/main here: {why}"))?;
    Ok(times.lines().next().and_then(|first| first.trim().parse().ok()))
}

fn snapshot(path: &Path) -> Snapshot {
    let written = std::fs::metadata(path).ok().and_then(|meta| meta.modified().ok()).and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok()).map(|age| age.as_secs_f64());
    Snapshot { written, failure: crate::threads::snapshot::last_failure(path) }
}

pub fn world(home: &Path, repo: &Path, now: f64) -> World {
    let mut hosts: Vec<(String, Result<HostFacts, String>)> = thread::scope(|scope| {
        let probes: Vec<_> = remote_hosts().into_iter().map(|(name, destination)| (name, scope.spawn(move || remote(&destination)))).collect();
        let local = facts::gather(home);
        let mut hosts = vec![(local.host.clone(), Ok(local))];
        hosts.extend(probes.into_iter().map(|(name, probe)| (name, probe.join().unwrap_or_else(|_| Err("the probe thread panicked".into())))));
        hosts
    });
    hosts.sort_by(|a, b| a.0.cmp(&b.0));
    let hosts = hosts
        .into_iter()
        .map(|(name, facts)| {
            let behind_since = facts.as_ref().map_err(String::clone).and_then(|facts| behind_since(repo, facts.runtime.as_deref()));
            Host { name, facts, behind_since }
        })
        .collect();
    let snapshot = home.join(".config/threads/remote").exists().then(|| snapshot(&home.join(".local/state/threads/snapshot.db")));
    World { now, hosts, snapshot }
}
