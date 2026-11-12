//! `north runtime probe services`: whether a host service here (a broker, a lander, a watchdog, the offload taker) runs
//! a runtime older than the one the judged runtime replaces. Each follows its link within a minute of a swap
//! (docs/rationale/runtime-follow.md), so one on the replaced runtime is on its way and one older than that follows no
//! promotion. A session's own processes, the sandbox broker its commands go through among them, move when their session
//! lets them, so one behind is counted and not a fault.
use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout};
use super::super::live::Swap;
use super::HOST_FAULT;
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
    let listed = Command::new(&north).args(LISTING).output()?;
    println!("north runtime probe services: {}", judge(layout, &judged, &String::from_utf8_lossy(&listed.stdout))?);
    Ok(())
}

/// Judges the processes `north runtime lag --processes --json` listed against the promotion of `judged` on this host: the
/// error names each host service on a runtime this host ran before the one `judged` replaces.
pub(super) fn judge(layout: &Layout, judged: &str, listing: &str) -> Result<String, Failure> {
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
    if services.is_empty() {
        let judged_services = processes.iter().filter(|process| !SESSION.contains(&process.kind.as_str())).count();
        return Ok(format!("none of {judged_services} host services runs a runtime older than {replaces}; {} session processes behind move with their sessions", sessions.len()));
    }
    let named: Vec<String> = services.iter().map(|(process, runtime)| format!("`{}` (pid {}) runs {}", process.process, process.pid, layout::short(runtime))).collect();
    Err(failed(HOST_FAULT, format!("host service {}: older than {replaces}, so it follows no promotion", named.join(", "))))
}
