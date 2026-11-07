use super::layout::{self, Layout};
use crate::platform::process;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const JOURNAL: &str = "runtime-live.jsonl";
const SEEN: &str = "runtime-live-seen.json";
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

pub(super) fn record(layout: &Layout, checkout: &Path, hosts: &[(String, String, u64)]) {
    let path = layout.state.join(SEEN);
    let mut seen: BTreeMap<String, String> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut rows = Vec::new();
    for (host, runtime, swapped_at) in hosts {
        let previous = seen.insert(host.clone(), runtime.clone());
        let Some(previous) = previous.filter(|previous| previous != runtime) else { continue };
        rows.extend(landed(checkout, &previous, runtime).into_iter().map(|(sha, landed_at)| Live { sha, host: host.clone(), landed_at, live_at: (*swapped_at as i64).max(landed_at) }));
    }
    let _ = fs::write(&path, serde_json::to_string(&seen).unwrap_or_default());
    let text: String = rows.iter().filter_map(|row| serde_json::to_string(row).ok()).map(|line| line + "\n").collect();
    if !text.is_empty() {
        let _ = fs::OpenOptions::new().create(true).append(true).open(journal(&layout.state)).and_then(|mut file| file.write_all(text.as_bytes()));
    }
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
    use super::on_main;
    use std::collections::HashMap;

    #[test]
    fn a_commit_lands_when_it_reaches_main_not_when_the_lander_commits_it() {
        let reached = HashMap::from([("9bd76afd".to_owned(), 1_794_017_717), ("1b19fc89".to_owned(), 1_794_017_597)]);
        let committed = [("c0ffee00".to_owned(), 1_794_017_700), ("9bd76afd".to_owned(), 1_794_017_631), ("ba5eba11".to_owned(), 1_794_017_560), ("1b19fc89".to_owned(), 1_794_017_546)];
        let landed: Vec<i64> = on_main(committed.into_iter(), &reached).into_iter().map(|(_, at)| at).collect();
        assert_eq!(landed, [1_794_017_700, 1_794_017_717, 1_794_017_717, 1_794_017_597]);
    }
}
