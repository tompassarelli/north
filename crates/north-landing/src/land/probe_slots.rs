use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::after::follow;

pub const MOST_AT_ONCE: usize = 6;
const MEASURED_PROBES: usize = 8;
const PROBE_LOG_SUFFIX: &str = "-candidate.log";

/// The free then total land checks the reachable build hosts take at once, read from `north capacity offload slots`: each host's free cores over the cores one check uses, capped at the checks it keeps for the lander.
pub fn free_slots(repo: &Path) -> Option<(usize, usize)> {
    let output = Command::new(follow::program()).args(["capacity", "offload", "slots"]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut words = text.split_whitespace().map(str::parse::<usize>);
    Some((words.next()?.ok()?, words.next()?.ok()?)).filter(|(free, total)| free <= total)
}

/// The probes one round may run side by side, and the free and total offload slots it read: the free slots over the offload jobs one probe takes, as the newest probe logs in `logs` show, so every job of every probe finds a slot.
pub fn slots(repo: &Path, logs: &Path) -> (usize, Option<(usize, usize)>) {
    let read = free_slots(repo);
    (read.map_or(MOST_AT_ONCE, |(free, _)| fitting(free, jobs_per_probe(logs))), read)
}

/// The probes of `jobs` offload jobs each that `free` slots hold at once, and always one.
pub fn fitting(free: usize, jobs: usize) -> usize {
    (free / jobs.max(1)).max(1)
}

/// The median count of offload jobs over the newest probe logs in `logs` that offloaded any, or 1 when none did: on 2026-11-08 one probe sent about 6 jobs to the build hosts, and planning one probe per slot queued 8 probes' jobs for 14 slots.
pub fn jobs_per_probe(logs: &Path) -> usize {
    let mut names: Vec<String> = fs::read_dir(logs).map(|entries| entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok()).filter(|name| is_probe_log(name)).collect()).unwrap_or_default();
    names.sort_unstable_by(|left, right| right.cmp(left));
    let mut counts: Vec<usize> = names.iter().filter_map(|name| fs::read_to_string(logs.join(name)).ok()).map(|text| offload_jobs(&text)).filter(|jobs| *jobs > 0).take(MEASURED_PROBES).collect();
    counts.sort_unstable();
    counts.get(counts.len() / 2).copied().unwrap_or(1)
}

fn is_probe_log(name: &str) -> bool {
    name.strip_suffix(PROBE_LOG_SUFFIX).and_then(|stem| stem.rsplit_once("-probe")).is_some_and(|(_, number)| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The offload jobs a land-check log started: each check offloaded alone, and each batch of head-north checks offloaded as one job.
pub fn offload_jobs(log: &str) -> usize {
    log.lines().filter(|line| line.strip_prefix("safe-push: offloading ").is_some_and(|rest| rest.starts_with("to ") || rest.ends_with(" as one job"))).count()
}
