use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const GIB: u64 = 1 << 30;
const CAP_BYTES: u64 = 10 * GIB;
const IDLE: Duration = Duration::from_secs(20 * 60);
const LIBRARY_EXTENSIONS: [&str; 5] = ["rlib", "rmeta", "d", "so", "a"];

struct Candidate {
    path: PathBuf,
    bytes: u64,
    touched: SystemTime,
}

pub(crate) fn run_sweep(cache: &Path) -> u8 {
    let mut status = 0;
    for target in shared_targets(cache) {
        match sweep(&target, CAP_BYTES, IDLE, SystemTime::now()) {
            Ok(freed) => println!("land-clean: swept {} ({} MiB freed)", target.display(), freed >> 20),
            Err(error) => {
                eprintln!("land-clean: could not sweep {}: {error}", target.display());
                status = 1;
            }
        }
    }
    status
}

pub(crate) fn sweep_cache(cache: &Path, now: SystemTime) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut trimmed = Vec::new();
    for target in shared_targets(cache) {
        let freed = sweep(&target, CAP_BYTES, IDLE, now)?;
        if freed > 0 {
            trimmed.push((target, freed));
        }
    }
    Ok(trimmed)
}

fn shared_targets(cache: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(cache) else { return Vec::new() };
    let mut targets: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name == "cargo-target" || name.starts_with("cargo-target-"))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    targets.sort();
    targets
}

fn sweep(target: &Path, cap: u64, idle: Duration, now: SystemTime) -> io::Result<u64> {
    let mut size = tree(target, &mut HashSet::new())?.0;
    let mut freed = 0;
    if size <= cap {
        return Ok(freed);
    }
    let mut candidates = stale_incremental(target, idle, now)?;
    candidates.extend(stale_executables(target, idle, now)?);
    candidates.sort_by_key(|candidate| candidate.touched);
    for candidate in candidates {
        if size <= cap {
            break;
        }
        let removed = if candidate.path.is_dir() {
            fs::remove_dir_all(&candidate.path)
        } else {
            fs::remove_file(&candidate.path)
        };
        if vanished(removed)?.is_some() {
            size = size.saturating_sub(candidate.bytes);
            freed += candidate.bytes;
        }
    }
    Ok(freed)
}

fn stale_incremental(target: &Path, idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    let mut stale = Vec::new();
    let Ok(entries) = fs::read_dir(target.join("debug/incremental")) else { return Ok(stale) };
    for entry in entries.flatten() {
        let Some((bytes, touched)) = vanished(tree(&entry.path(), &mut HashSet::new()))? else { continue };
        if idle_since(touched, idle, now) {
            stale.push(Candidate { path: entry.path(), bytes, touched });
        }
    }
    Ok(stale)
}

fn stale_executables(target: &Path, idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    let mut stale = Vec::new();
    let Ok(entries) = fs::read_dir(target.join("debug/deps")) else { return Ok(stale) };
    for entry in entries.flatten() {
        let Some(metadata) = vanished(fs::symlink_metadata(entry.path()))? else { continue };
        let library = entry
            .path()
            .extension()
            .is_some_and(|extension| LIBRARY_EXTENSIONS.iter().any(|kept| extension == *kept));
        let executable = metadata.is_file() && metadata.mode() & 0o111 != 0;
        let touched = metadata.modified()?;
        if executable && !library && metadata.nlink() == 1 && idle_since(touched, idle, now) {
            stale.push(Candidate { path: entry.path(), bytes: metadata.len(), touched });
        }
    }
    Ok(stale)
}

fn idle_since(touched: SystemTime, idle: Duration, now: SystemTime) -> bool {
    now.duration_since(touched).is_ok_and(|age| age >= idle)
}

fn vanished<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn tree(path: &Path, seen: &mut HashSet<(u64, u64)>) -> io::Result<(u64, SystemTime)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        let linked_again = metadata.nlink() > 1 && !seen.insert((metadata.dev(), metadata.ino()));
        return Ok((if linked_again { 0 } else { metadata.len() }, metadata.modified()?));
    }
    let mut newest = SystemTime::UNIX_EPOCH;
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let Some((bytes, touched)) = vanished(tree(&entry?.path(), seen))? else { continue };
        total += bytes;
        newest = newest.max(touched);
    }
    Ok((total, newest))
}

#[cfg(test)]
#[path = "shared_target_tests.rs"]
mod tests;
