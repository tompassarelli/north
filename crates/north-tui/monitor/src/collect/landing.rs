use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use north_base::platform::land_queue::{self, queued_seconds, repo_name, tickets_in};
use serde::Deserialize;

use crate::snapshot::{Decided, Lander, Landing};

const WINDOW_S: u64 = 26 * 3600;
const RECENT: usize = 8;

#[derive(Deserialize)]
struct Ticket {
    #[serde(default)]
    origin: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    verdict: Option<String>,
}

#[derive(Clone)]
struct Done {
    repo: String,
    branch: String,
    verdict: String,
    queued: u64,
    decided: u64,
}

#[derive(Default)]
pub struct Cache {
    done: HashMap<PathBuf, Option<Done>>,
}

fn decided(path: &Path, name: &str) -> Option<Done> {
    let decided = fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let ticket: Ticket = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some(Done { repo: repo_name(&ticket.origin), branch: ticket.branch, verdict: ticket.verdict.unwrap_or_default(), queued: queued_seconds(name)?, decided })
}

fn percentile(sorted: &[u64], fraction: f64) -> Option<u64> {
    let last = sorted.len().checked_sub(1)?;
    sorted.get(((last as f64) * fraction).round() as usize).copied()
}

pub fn read(queue: &Path, at: u64, cache: &mut Cache) -> Result<Landing, String> {
    let repos = fs::read_dir(queue).map_err(|error| format!("{}: {error}", queue.display()))?;
    let pending: Vec<u64> = land_queue::waiting(queue, at as f64).iter().map(|ticket| ticket.queued).collect();
    let mut done: Vec<Done> = Vec::new();
    for repo in repos.flatten().filter(|entry| entry.path().is_dir()) {
        for (path, name) in tickets_in(&repo.path().join("done")) {
            let Some(queued) = queued_seconds(&name) else { continue };
            if queued + WINDOW_S < at {
                continue;
            }
            let entry = cache.done.entry(path).or_insert_with_key(|path| decided(path, &name));
            done.extend(entry.clone());
        }
    }
    let depth = |when: u64| -> u32 {
        let waiting = pending.iter().filter(|queued| **queued <= when).count() + done.iter().filter(|ticket| ticket.queued <= when && ticket.decided > when).count();
        waiting as u32
    };
    let seen: Vec<&Done> = done.iter().filter(|ticket| ticket.decided <= at).collect();
    let landed = |ticket: &&&Done| ticket.verdict.starts_with("landed");
    let mut times: Vec<u64> = seen.iter().filter(landed).filter(|ticket| ticket.decided + 3600 > at).map(|ticket| ticket.decided.saturating_sub(ticket.queued)).collect();
    times.sort_unstable();
    let six_hours: Vec<&&Done> = seen.iter().filter(|ticket| ticket.decided + 6 * 3600 > at && !ticket.verdict.starts_with("replaced")).collect();
    let mut recent: Vec<&&Done> = seen.iter().collect();
    recent.sort_by_key(|ticket| std::cmp::Reverse(ticket.decided));
    let landed_per_hour = (0..24u64)
        .rev()
        .map(|hours_ago| seen.iter().filter(landed).filter(|ticket| ticket.decided + (hours_ago + 1) * 3600 > at && ticket.decided + hours_ago * 3600 <= at).count() as u32)
        .collect();
    Ok(Landing {
        depth: depth(at),
        depth_5m_ago: depth(at.saturating_sub(300)),
        land_p50_s: percentile(&times, 0.5),
        land_p95_s: percentile(&times, 0.95),
        landed_last_hour: times.len() as u32,
        decided_6h: six_hours.len() as u32,
        failed_6h: six_hours.iter().filter(|ticket| ticket.verdict.starts_with("failed")).count() as u32,
        recent: recent
            .into_iter()
            .take(RECENT)
            .map(|ticket| Decided { repo: ticket.repo.clone(), branch: ticket.branch.clone(), verdict: ticket.verdict.clone(), at: ticket.decided, seconds: ticket.decided.saturating_sub(ticket.queued) })
            .collect(),
        depth_trend: (0..30u64).rev().map(|step| depth(at.saturating_sub(step * 120))).collect(),
        landed_per_hour,
    })
}

const LOCK_FRESH_S: u64 = 6 * 3600;

fn lander(lock: &Path, at: u64) -> Option<Lander> {
    let text = fs::read_to_string(lock).ok()?;
    let mut words = text.split_whitespace();
    let pid = words.nth(1)?;
    let served = words.nth(1)?;
    let repo = served.split("/code/").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or(served).to_owned();
    let command = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let command = String::from_utf8_lossy(&command).replace('\0', " ");
    let up = command.contains("land serve");
    let written = fs::metadata(lock).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    if !up && written + LOCK_FRESH_S < at {
        return None;
    }
    let build = command.split("runtimes/").nth(1).map(|rest| rest.chars().take(7).collect());
    Some(Lander { repo, up, build })
}

pub fn landers(queue: &Path, at: u64) -> Result<Vec<Lander>, String> {
    let repos = fs::read_dir(queue).map_err(|error| format!("{}: {error}", queue.display()))?;
    let mut found: Vec<Lander> = repos.flatten().filter_map(|repo| lander(&repo.path().join("lander.lock"), at)).collect();
    found.sort_by(|left, right| left.up.cmp(&right.up).then_with(|| left.repo.cmp(&right.repo)));
    Ok(found)
}
