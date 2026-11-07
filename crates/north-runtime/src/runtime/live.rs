use super::layout::{self, Layout};
use crate::platform::process;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const JOURNAL: &str = "runtime-live.jsonl";
const SEEN: &str = "runtime-live-seen.json";
pub(super) const SWAPS_KEPT: usize = 200;
const ACTIVE_S: i64 = 86_400;
const ORIGIN_MAIN: &str = "refs/remotes/origin/main";

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub sha: String,
    pub host: String,
    pub landed_at: i64,
    pub live_at: i64,
}

pub fn journal(state: &Path) -> PathBuf {
    state.join(JOURNAL)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub(super) struct Swap {
    pub(super) runtime: String,
    pub(super) at: i64,
}

pub(super) fn note_swap(layout: &Layout, link: &Path) {
    let Some(runtime) = layout::linked_sha(link) else { return };
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64);
    let Ok(line) = serde_json::to_string(&Swap { runtime: runtime.to_owned(), at }) else { return };
    let _ = fs::OpenOptions::new().create(true).append(true).open(layout.swaps()).and_then(|mut file| writeln!(file, "{line}"));
}

pub(super) fn record(layout: &Layout, checkout: &Path, hosts: &[(String, Vec<Swap>)]) {
    let path = layout.state.join(SEEN);
    let mut seen: BTreeMap<String, String> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut rows = Vec::new();
    for (host, swaps) in hosts {
        let moves = moves_since(swaps, seen.get(host).map(String::as_str));
        rows.extend(moves.into_iter().flat_map(|(previous, swap)| landed(checkout, previous, &swap.runtime).into_iter().map(|(sha, landed_at)| Live { sha, host: host.clone(), landed_at, live_at: swap.at.max(landed_at) })));
        if let Some(last) = swaps.last() {
            seen.insert(host.clone(), last.runtime.clone());
        }
    }
    let _ = fs::write(&path, serde_json::to_string(&seen).unwrap_or_default());
    let text: String = rows.iter().filter_map(|row| serde_json::to_string(row).ok()).map(|line| line + "\n").collect();
    if !text.is_empty() {
        let _ = fs::OpenOptions::new().create(true).append(true).open(journal(&layout.state)).and_then(|mut file| file.write_all(text.as_bytes()));
    }
}

fn moves_since<'a>(swaps: &'a [Swap], seen: Option<&'a str>) -> Vec<(&'a str, &'a Swap)> {
    let unseen = seen.and_then(|seen| swaps.iter().rposition(|swap| swap.runtime == seen)).map_or(swaps, |at| &swaps[at + 1..]);
    let mut previous = seen;
    let mut moves = Vec::new();
    for swap in unseen {
        if let Some(from) = previous.filter(|from| *from != swap.runtime) {
            moves.push((from, swap));
        }
        previous = Some(&swap.runtime);
    }
    moves
}

fn landed(checkout: &Path, previous: &str, runtime: &str) -> Vec<(String, i64)> {
    let range = format!("{previous}..{runtime}");
    let Ok(output) = process::run("git", Some(checkout), &["rev-list", "--first-parent", "--format=%H %ct", "--no-commit-header", &range]) else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let committed = text.lines().filter_map(|line| line.split_once(' ')).filter(|(sha, _)| layout::is_sha(sha)).filter_map(|(sha, at)| Some((sha.to_owned(), at.trim().parse().ok()?)));
    on_main(committed, &reached(checkout))
}

fn reached(checkout: &Path) -> HashMap<String, i64> {
    let Ok(output) = process::run("git", Some(checkout), &["log", "--walk-reflogs", "--date=unix", "--format=%H %gd", ORIGIN_MAIN]) else { return HashMap::new() };
    let mut reached = HashMap::new();
    for (sha, at) in String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.split_once(' ')).filter_map(|(sha, selector)| Some((sha.to_owned(), selector.rsplit_once("@{")?.1.strip_suffix('}')?.parse::<i64>().ok()?))) {
        let earliest = reached.entry(sha).or_insert(at);
        *earliest = (*earliest).min(at);
    }
    reached
}

fn on_main(committed: impl Iterator<Item = (String, i64)>, reached: &HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut earliest: Option<i64> = None;
    committed
        .map(|(sha, committed_at)| {
            earliest = earliest.into_iter().chain(reached.get(&sha).copied()).min();
            let landed_at = earliest.map_or(committed_at, |at| at.max(committed_at));
            (sha, landed_at)
        })
        .collect()
}

pub fn everywhere(rows: &[Live]) -> Vec<(i64, f64)> {
    let newest = rows.iter().map(|row| row.live_at).max().unwrap_or(0);
    let mut last: HashMap<&str, i64> = HashMap::new();
    for row in rows {
        let at = last.entry(row.host.as_str()).or_insert(row.live_at);
        *at = (*at).max(row.live_at);
    }
    let hosts = last.values().filter(|at| **at >= newest - ACTIVE_S).count();
    let mut landings: HashMap<&str, (i64, i64, usize)> = HashMap::new();
    for row in rows {
        let entry = landings.entry(row.sha.as_str()).or_insert((row.landed_at, row.live_at, 0));
        entry.1 = entry.1.max(row.live_at);
        entry.2 += 1;
    }
    landings.into_values().filter(|(_, _, count)| *count >= hosts).map(|(landed_at, live_at, _)| (landed_at, (live_at - landed_at) as f64)).collect()
}

pub fn read(state: &Path) -> Vec<Live> {
    fs::read_to_string(journal(state)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::{Swap, moves_since, on_main};
    use std::collections::HashMap;

    #[test]
    fn a_commit_lands_when_it_reaches_main_not_when_the_lander_commits_it() {
        let reached = HashMap::from([("9bd76afd".to_owned(), 1_794_017_717), ("1b19fc89".to_owned(), 1_794_017_597)]);
        let committed = [("c0ffee00".to_owned(), 1_794_017_700), ("9bd76afd".to_owned(), 1_794_017_631), ("ba5eba11".to_owned(), 1_794_017_560), ("1b19fc89".to_owned(), 1_794_017_546)];
        let landed: Vec<i64> = on_main(committed.into_iter(), &reached).into_iter().map(|(_, at)| at).collect();
        assert_eq!(landed, [1_794_017_700, 1_794_017_717, 1_794_017_717, 1_794_017_597]);
    }

    #[test]
    fn a_landing_a_newer_one_supersedes_before_the_next_probe_keeps_its_own_swap_time() {
        let swap = |runtime: &str, at| Swap { runtime: runtime.to_owned(), at };
        let swaps = [swap("aaaa", 100), swap("f640", 200), swap("cf56", 300)];
        let moves: Vec<(&str, i64)> = moves_since(&swaps, Some("aaaa")).into_iter().map(|(from, swap)| (from, swap.at)).collect();
        assert_eq!(moves, [("aaaa", 200), ("f640", 300)]);
        assert!(moves_since(&swaps, Some("cf56")).is_empty());
    }
}
