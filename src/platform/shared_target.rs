use std::collections::HashSet;
use std::fs::{self, File, TryLockError};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const GIB: u64 = 1 << 30;
const CAP_BYTES: u64 = 10 * GIB;
const IDLE: Duration = Duration::from_secs(20 * 60);
const BUILD_LOCK: &str = ".cargo-lock";
const PROFILES:[&str; 2] = ["debug", "release"];
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
    for parent in targets.clone() {
        let Ok(children) = fs::read_dir(&parent) else { continue };
        targets.extend(children.flatten().filter(|child| holds_profile(&child.path())).map(|child| child.path()));
    }
    targets.sort();
    targets
}

fn holds_profile(directory: &Path) -> bool {
    directory.is_dir() && PROFILES.iter().any(|profile| directory.join(profile).is_dir())
}

fn profile_size(target: &Path) -> io::Result<u64> {
    let mut size = 0;
    for profile in PROFILES {
        size += vanished(allocated_tree(&target.join(profile), &mut HashSet::new()))?.map_or(0, |(bytes, _)| bytes);
    }
    Ok(size)
}

fn sweep(target: &Path, cap: u64, idle: Duration, now: SystemTime) -> io::Result<u64> {
    let mut size = profile_size(target)?;
    let mut freed = 0;
    if size <= cap {
        return Ok(freed);
    }
    let mut transient = Vec::new();
    for profile in PROFILES {
        transient.extend(stale_incremental(target, profile, idle, now)?);
        transient.extend(stale_executables(target, profile, idle, now)?);
    }
    freed += trim(transient, cap, &mut size)?;
    if size > cap {
        let (_locks, unlocked) = lock_profiles(target)?;
        let mut libraries = Vec::new();
        for &profile in &unlocked {
            libraries.extend(stale_libraries(target, profile, idle, now)?);
        }
        freed += trim(libraries, cap, &mut size)?;
        if size > cap {
            freed += trim(stale_profiles(target, &unlocked, idle, now)?, cap, &mut size)?;
        }
    }
    Ok(freed)
}

fn trim(mut candidates: Vec<Candidate>, cap: u64, size: &mut u64) -> io::Result<u64> {
    candidates.sort_by_key(|candidate| candidate.touched);
    let mut freed = 0;
    for candidate in candidates {
        if *size <= cap {
            break;
        }
        freed += remove(&candidate, size)?;
    }
    Ok(freed)
}

fn remove(candidate: &Candidate, size: &mut u64) -> io::Result<u64> {
    let removed = if candidate.path.is_dir() {
        fs::remove_dir_all(&candidate.path)
    } else {
        fs::remove_file(&candidate.path)
    };
    if vanished(removed)?.is_none() {
        return Ok(0);
    }
    *size = size.saturating_sub(candidate.bytes);
    Ok(candidate.bytes)
}

fn stale_profiles(target: &Path, profiles: &[&str], idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    let mut stale = Vec::new();
    for profile in profiles {
        let path = target.join(profile);
        let Some((bytes, touched)) = vanished(allocated_tree(&path, &mut HashSet::new()))? else { continue };
        if idle_since(touched, idle, now) {
            stale.push(Candidate { path, bytes, touched });
        }
    }
    Ok(stale)
}

fn stale_incremental(target: &Path, profile: &str, idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    let mut stale = Vec::new();
    let Ok(entries) = fs::read_dir(target.join(profile).join("incremental")) else { return Ok(stale) };
    for entry in entries.flatten() {
        let Some((bytes, touched)) = vanished(allocated_tree(&entry.path(), &mut HashSet::new()))? else { continue };
        if idle_since(touched, idle, now) {
            stale.push(Candidate { path: entry.path(), bytes, touched });
        }
    }
    Ok(stale)
}

fn stale_executables(target: &Path, profile: &str, idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    stale_dependency_files(target, profile, idle, now, |metadata, library| {
        metadata.is_file() && metadata.mode() & 0o111 != 0 && !library
    })
}

fn lock_profiles(target: &Path) -> io::Result<(Vec<File>, Vec<&'static str>)> {
    let mut locks = Vec::new();
    let mut unlocked = Vec::new();
    for profile in PROFILES {
        let directory = target.join(profile);
        if !directory.is_dir() {
            continue;
        }
        if hold_build_lock(&directory, &mut locks)? {
            unlocked.push(profile);
        } else {
            eprintln!("land-clean: skipped {profile} in {}: a build holds its {BUILD_LOCK}", target.display());
        }
    }
    Ok((locks, unlocked))
}

fn hold_build_lock(profile: &Path, locks: &mut Vec<File>) -> io::Result<bool> {
    let Some(lock) = vanished(File::open(profile.join(BUILD_LOCK)))? else { return Ok(true) };
    match lock.try_lock() {
        Ok(()) => {
            locks.push(lock);
            Ok(true)
        }
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn stale_libraries(target: &Path, profile: &str, idle: Duration, now: SystemTime) -> io::Result<Vec<Candidate>> {
    stale_dependency_files(target, profile, idle, now, |metadata, library| metadata.is_file() && library)
}

fn stale_dependency_files(
    target: &Path,
    profile: &str,
    idle: Duration,
    now: SystemTime,
    wanted: impl Fn(&fs::Metadata, bool) -> bool,
) -> io::Result<Vec<Candidate>> {
    let mut stale = Vec::new();
    let Ok(entries) = fs::read_dir(target.join(profile).join("deps")) else { return Ok(stale) };
    for entry in entries.flatten() {
        let Some(metadata) = vanished(fs::symlink_metadata(entry.path()))? else { continue };
        let library = entry
            .path()
            .extension()
            .is_some_and(|extension| LIBRARY_EXTENSIONS.iter().any(|kept| extension == *kept));
        let touched = metadata.modified()?;
        if wanted(&metadata, library) && metadata.nlink() == 1 && idle_since(touched, idle, now) {
            stale.push(Candidate { path: entry.path(), bytes: disk_bytes(&metadata), touched });
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

fn disk_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.len().max(metadata.blocks() * 512)
}

pub(crate) fn tree(path: &Path, seen: &mut HashSet<(u64, u64)>) -> io::Result<(u64, SystemTime)> {
    measured_tree(path, seen, fs::Metadata::len)
}

fn allocated_tree(path: &Path, seen: &mut HashSet<(u64, u64)>) -> io::Result<(u64, SystemTime)> {
    measured_tree(path, seen, disk_bytes)
}

fn measured_tree(
    path: &Path,
    seen: &mut HashSet<(u64, u64)>,
    measure: fn(&fs::Metadata) -> u64,
) -> io::Result<(u64, SystemTime)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        let linked_again = metadata.nlink() > 1 && !seen.insert((metadata.dev(), metadata.ino()));
        return Ok((if linked_again { 0 } else { measure(&metadata) }, metadata.modified()?));
    }
    let mut newest = SystemTime::UNIX_EPOCH;
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let Some((bytes, touched)) = vanished(measured_tree(&entry?.path(), seen, measure))? else { continue };
        total += bytes;
        newest = newest.max(touched);
    }
    Ok((total, newest))
}

#[cfg(test)]
#[path = "shared_target_tests.rs"]
mod tests;
