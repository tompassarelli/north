//! `north runtime probe services`: whether a host service here (a broker, a lander, a watchdog, the offload taker) runs
//! a runtime older than the one the judged runtime replaces. Each follows its link within a minute of a swap
//! (docs/rationale/runtime-follow.md), so one on the replaced runtime is on its way and one older than that follows no
//! promotion. A session's own processes, the sandbox broker its commands go through among them, move when their session
//! lets them, so one behind is counted and not a fault. A service a client started and no unit restarts is first asked
//! to exit while idle (retirement.rs), and one still serving a client that no new client reaches is leaving already, so
//! the probe names it and fails only on what a promotion cannot move.
use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout};
use super::super::live::Swap;
use super::{HOST_FAULT, retirement};
use north_base::foundation::exit::NO_VERDICT;
use north_runtime_follow::{named, runtime_of};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

const SESSION: [&str; 4] = ["session-host", "session-link", "tool-server", "worker-sandbox broker"];
const LANDER: &str = "land serve";
const LISTING: [&str; 4] = ["runtime", "lag", "--processes", "--json"];

#[derive(Deserialize)]
struct Process {
    pid: u32,
    #[serde(default)]
    kind: String,
    process: String,
    runtime: Option<String>,
}

pub(in crate::runtime) fn probe(layout: &Layout) -> Result<(), Failure> {
    let north = std::env::current_exe()?;
    let judged = runtime_of(&north).map(|(sha, _)| sha).or_else(|| layout.live_sha()).ok_or_else(|| failed(NO_VERDICT, "this north runs from no runtime and the host has none live, so no host service is judged"))?;
    let draining = asked(layout, &judged);
    println!("north runtime probe services: {}", judge(layout, &judged, &listed(&north)?, &draining)?);
    Ok(())
}

/// A promotion of `judged` asks each on-demand host service here on a runtime older than the live one to exit once idle, before its canary judges what is left and again once `judged` is live.
pub(in crate::runtime) fn retire(layout: &Layout, judged: &str) {
    asked(layout, judged);
}

/// The pids of the services asked that still serve a client and that no new client reaches.
fn asked(layout: &Layout, judged: &str) -> Vec<u32> {
    let Some(north) = std::env::current_exe().ok().filter(|north| runtime_of(north).is_some()) else {
        return Vec::new();
    };
    match listed(&north).map(|listing| retirement::retire_here(layout.live_sha().as_deref(), judged, &listing)) {
        Ok(asked) => {
            asked.said.iter().for_each(|line| println!("north runtime: {line}"));
            asked.draining
        }
        Err(error) => {
            println!("north runtime: no on-demand host service was asked to exit, since the processes here could not be listed: {error}");
            Vec::new()
        }
    }
}

fn listed(north: &Path) -> std::io::Result<String> {
    Command::new(north).args(LISTING).output().map(|listed| String::from_utf8_lossy(&listed.stdout).into_owned())
}

/// Judges the processes `north runtime lag --processes --json` listed against the promotion of `judged` on this host: the
/// error names each host service on a runtime this host ran before the one `judged` replaces, but for the `draining` ones.
pub(super) fn judge(layout: &Layout, judged: &str, listing: &str, draining: &[u32]) -> Result<String, Failure> {
    let processes: Vec<Process> = serde_json::from_str(listing.trim()).map_err(|error| failed(NO_VERDICT, format!("`north runtime lag --processes --json` listed no processes, so no host service is judged: {error}")))?;
    let swapped: Vec<String> = fs::read_to_string(layout.swaps()).unwrap_or_default().lines().filter_map(|line| serde_json::from_str::<Swap>(line).ok()).map(|swap| swap.runtime).collect();
    let live = layout.live_sha();
    let replaced = match swapped.iter().rposition(|runtime| runtime == judged) {
        Some(went_live) => swapped[..went_live].iter().rev().find(|runtime| *runtime != judged).cloned(),
        None => live.clone().filter(|live| live != judged),
    };
    let lander: Vec<String> = named(&layout.lander()).into_iter().chain(fs::read_to_string(layout.lander_rollback()).ok().and_then(|link| layout::linked_sha(Path::new(link.trim())).map(str::to_owned))).collect();
    let follows = |process: &Process, runtime: &str| [Some(judged), live.as_deref(), replaced.as_deref()].into_iter().flatten().chain(lander.iter().map(String::as_str).filter(|_| process.kind == LANDER)).any(|followed| followed == runtime);
    let ran_here = |runtime: &str| swapped.iter().any(|swapped| swapped == runtime) || layout.runtime(runtime).is_dir();
    let behind: Vec<(&Process, &str)> = processes.iter().filter_map(|process| process.runtime.as_deref().filter(|runtime| !follows(process, runtime) && ran_here(runtime)).map(|runtime| (process, runtime))).collect();
    let (sessions, services): (Vec<_>, Vec<_>) = behind.iter().partition(|(process, _)| SESSION.contains(&process.kind.as_str()));
    let short = layout::short(judged);
    let replaces = replaced.as_deref().map_or_else(|| format!("{short}, which replaces no runtime on record here"), |replaced| format!("{}, the runtime {short} replaces", layout::short(replaced)));
    let (leaving, services): (Vec<_>, Vec<_>) = services.into_iter().partition(|(process, _)| draining.contains(&process.pid));
    if services.is_empty() {
        let judged_services = processes.iter().filter(|process| !SESSION.contains(&process.kind.as_str())).count();
        let left: String = leaving.iter().map(|(process, runtime)| format!("; `{}` (pid {}) on {} takes no new client and leaves with its last one", process.process, process.pid, layout::short(runtime))).collect();
        return Ok(format!("none of {judged_services} host services runs a runtime older than {replaces} and still takes a new client; {} session processes behind move with their sessions{left}", sessions.len()));
    }
    let named: Vec<String> = services.iter().map(|(process, runtime)| format!("`{}` (pid {}) runs {}", process.process, process.pid, layout::short(runtime))).collect();
    Err(failed(HOST_FAULT, format!("host service {}: older than {replaces}, so it follows no promotion", named.join(", "))))
}
