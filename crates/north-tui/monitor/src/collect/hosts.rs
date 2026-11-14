use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use north_base::platform::host_load::short;
use serde::Deserialize;

use super::run;

pub fn host_name(name: &str) -> &str {
    name.rsplit('@').next().unwrap_or(name)
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
