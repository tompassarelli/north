use std::path::{Path, PathBuf};

use super::exec::{command, git, real};
use super::liveness::Trees;
use super::super::live_process::{any_inside, open_paths};
use crate::foundation::clock::Timestamp;

const DAY_MS: i64 = 86_400_000;

pub(super) fn scan(root: &Path, now_ms: i64) -> Trees {
    let mut trees = Trees { ok: true, branches: Default::default() };
    if scan_into(root, now_ms, &mut trees).is_err() {
        trees.ok = false;
    }
    trees
}

pub(super) fn scan_remote(now_ms: i64) -> Trees {
    let ran = command(&["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", "~/.local/state/north/current/north", "lane-gc", "--scan-trees", "~/code", &now_ms.to_string()]);
    ran.ok.then(|| serde_json::from_str(&ran.out).ok()).flatten().unwrap_or_default()
}

pub(super) fn expand(root: &str, home: &Path) -> PathBuf {
    root.strip_prefix("~/").map_or_else(|| PathBuf::from(root), |rest| home.join(rest))
}

fn checked(dir: &Path, args: &[&str]) -> Result<String, ()> {
    let ran = git(dir, args);
    if ran.ok { Ok(ran.out) } else { Err(()) }
}

fn scan_into(root: &Path, now_ms: i64, trees: &mut Trees) -> Result<(), ()> {
    let live = open_paths(Path::new("/proc"));
    let cutoff = Timestamp::from_unix_millis(now_ms - DAY_MS).iso_millis();
    for container in std::fs::read_dir(root).map_err(|_| ())?.flatten().map(|entry| entry.path()) {
        let (main, lanes) = (container.join("main"), container.join("worktrees"));
        if !main.join(".git").exists() || !lanes.exists() {
            continue;
        }
        let lanes = real(&lanes);
        let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for block in checked(&main, &["worktree", "list", "--porcelain"])?.split("\n\n") {
            let field = |key: &str| block.lines().find_map(|line| if line == key { Some("") } else { line.strip_prefix(key)?.strip_prefix(' ') });
            let path = PathBuf::from(field("worktree").unwrap_or("/"));
            let branch = field("branch").unwrap_or("").trim_start_matches("refs/heads/");
            if path.parent().map(real) != Some(lanes.clone()) || branch.is_empty() {
                continue;
            }
            let key = format!("{name}:{branch}");
            let shown = path.display();
            let why = if field("locked").is_some() {
                "locked worktree".to_string()
            } else if !path.exists() {
                continue;
            } else if any_inside(&live, &path) {
                format!("process in {shown}")
            } else if !checked(&path, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty() {
                format!("dirty {shown}")
            } else if !found(&path, &cutoff)?.is_empty() {
                format!("file modified under 24h in {shown}")
            } else {
                continue;
            };
            trees.branches.insert(key, why);
        }
    }
    Ok(())
}

fn found(path: &Path, cutoff: &str) -> Result<String, ()> {
    let ran = command(&["find", &path.to_string_lossy(), "-type", "f", "-newermt", cutoff, "-print", "-quit"]);
    if ran.ok { Ok(ran.out) } else { Err(()) }
}
