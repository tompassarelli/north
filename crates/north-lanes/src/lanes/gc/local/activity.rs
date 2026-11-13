use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub(in crate::lanes) fn parse_worktrees(porcelain: &str) -> Vec<BTreeMap<String, String>> {
    porcelain
        .split("\n\n")
        .map(|block| block.lines().filter(|line| !line.is_empty()).map(|line| line.split_once(' ').map_or((line.into(), "true".into()), |(k, v)| (k.into(), v.into()))).collect::<BTreeMap<String, String>>())
        .filter(|fields| fields.contains_key("worktree"))
        .collect()
}

pub(in crate::lanes) fn branch_of(fields: &BTreeMap<String, String>) -> String {
    fields.get("branch").map(|branch| branch.trim_start_matches("refs/heads/").to_string()).unwrap_or_default()
}

fn newest_mtime(paths: &[PathBuf]) -> f64 {
    paths
        .iter()
        .filter_map(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .filter_map(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64() * 1000.0)
        .fold(0.0, f64::max)
}

pub(in crate::lanes) fn reflog_ms(path: &Path) -> f64 {
    let Ok(text) = std::fs::read_to_string(path) else { return 0.0 };
    let stamp = |line: &str| line.split('\t').next()?.split_whitespace().rev().nth(1)?.parse::<u64>().ok();
    text.lines().filter_map(stamp).max().map_or(0.0, |secs| secs as f64 * 1000.0)
}

pub(in crate::lanes) fn lane_activity_ms(path: &Path, admin: &Path, common: &Path, branch: &str, committed_ms: f64) -> f64 {
    let mut stamps = vec![reflog_ms(&admin.join("logs/HEAD")), newest_mtime(&[path.to_path_buf(), admin.join("HEAD"), admin.join("index")])];
    if !branch.is_empty() {
        stamps.push(reflog_ms(&common.join("logs/refs/heads").join(branch)));
    }
    stamps.into_iter().fold(committed_ms, f64::max)
}
