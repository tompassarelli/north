//! Stale git lock files: the one removal an agent may make in a primary checkout's git directory, run by `north lane unlock` and by every north path that fetches or fast-forwards a checkout (north#937). It depends on std alone.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use std::process::Command;

pub const STALE_AFTER: Duration = Duration::from_secs(60);
const RACE_RETRIES: u32 = 8;
const RACE_PAUSE: Duration = Duration::from_millis(250);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    pub removed: Vec<PathBuf>,
    pub kept: Vec<(PathBuf, Kept)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Kept {
    Held { pid: u32 },
    Fresh { age: Duration },
}

impl fmt::Display for Sweep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for lock in &self.removed {
            writeln!(formatter, "removed {}: no process holds it and it is older than {} s", lock.display(), STALE_AFTER.as_secs())?;
        }
        for (lock, kept) in &self.kept {
            match kept {
                Kept::Held { pid } => writeln!(formatter, "kept {}: process {pid} holds it", lock.display())?,
                Kept::Fresh { age } => writeln!(formatter, "kept {}: {} s old, younger than {} s", lock.display(), age.as_secs(), STALE_AFTER.as_secs())?,
            }
        }
        Ok(())
    }
}

/// Removes each git lock file in `checkout`'s git directory and common directory that no process holds open and that is older than `STALE_AFTER`, the only removal an agent may make in a primary checkout's git directory.
pub fn clear(checkout: &Path) -> Result<Sweep, String> {
    clear_at(checkout, SystemTime::now())
}

/// Runs `operation`; when it fails because a sibling worktree moved the same remote-tracking ref first, runs it again a bounded number of times, and when it fails naming a lock file, clears the stale locks in `checkout` and runs it once more if any was removed.
pub fn retry_after_clearing<T>(checkout: &Path, mut operation: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    let mut result = operation();
    for _ in 0..RACE_RETRIES {
        if !result.as_ref().is_err_and(|error| lost_ref_race(error)) {
            break;
        }
        std::thread::sleep(RACE_PAUSE);
        result = operation();
    }
    match result {
        Err(error) if error.contains(".lock") => match clear(checkout) {
            Ok(sweep) if !sweep.removed.is_empty() => {
                eprint!("{sweep}");
                operation()
            }
            Ok(sweep) => Err(format!("{error}\n{sweep}").trim_end().to_owned()),
            Err(_) => Err(error),
        },
        done => done,
    }
}

fn lost_ref_race(error: &str) -> bool {
    error.contains("cannot lock ref") && error.contains("but expected")
}

fn clear_at(checkout: &Path, now: SystemTime) -> Result<Sweep, String> {
    let locks: BTreeSet<PathBuf> = git_dirs(checkout)?.iter().flat_map(|dir| locks_in(dir)).collect();
    let mut sweep = Sweep::default();
    if locks.is_empty() {
        return Ok(sweep);
    }
    let holders = holders(&locks);
    for lock in locks {
        if let Some(pid) = holders.get(&lock) {
            sweep.kept.push((lock, Kept::Held { pid: *pid }));
            continue;
        }
        let Ok(modified) = std::fs::metadata(&lock).and_then(|meta| meta.modified()) else {
            continue;
        };
        let age = now.duration_since(modified).unwrap_or_default();
        if age < STALE_AFTER {
            sweep.kept.push((lock, Kept::Fresh { age }));
            continue;
        }
        match std::fs::remove_file(&lock) {
            Ok(()) => sweep.removed.push(lock),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot remove {}: {error}", lock.display())),
        }
    }
    Ok(sweep)
}

fn git_dirs(checkout: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let output = Command::new("git").arg("-C").arg(checkout).args(["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir"]).output().map_err(|error| format!("cannot run git in {}: {error}", checkout.display()))?;
    if !output.status.success() {
        return Err(format!("{} is not a git checkout: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    let listed = output.stdout;
    Ok(String::from_utf8_lossy(&listed).lines().filter(|line| !line.is_empty()).filter_map(|line| std::fs::canonicalize(line).ok()).collect())
}

fn locks_in(dir: &Path) -> Vec<PathBuf> {
    let mut locks: Vec<PathBuf> = entries(dir).chain(entries(&dir.join("packed"))).filter(|path| path.is_file() && owned_by_git(path)).collect();
    let mut pending = vec![dir.join("refs")];
    while let Some(next) = pending.pop() {
        for path in entries(&next) {
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "lock") {
                locks.push(path);
            }
        }
    }
    locks
}

fn entries(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path())
}

fn owned_by_git(path: &Path) -> bool {
    let Some(stem) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".lock")) else {
        return false;
    };
    matches!(stem, "index" | "config" | "packed-refs" | "shallow" | "commit-graph") || (!stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_'))
}

fn holders(locks: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, u32> {
    let mut holders = BTreeMap::new();
    for process in entries(Path::new("/proc")) {
        let Some(pid) = process.file_name().and_then(|name| name.to_str()).and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        for fd in entries(&process.join("fd")) {
            if let Ok(target) = std::fs::read_link(&fd)
                && locks.contains(&target)
            {
                holders.insert(target, pid);
            }
        }
    }
    holders
}

#[cfg(test)]
mod tests;
