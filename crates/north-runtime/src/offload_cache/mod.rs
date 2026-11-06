//! Keeps a build host's offload slots from filling its root disk: the least recently used slot whose lock no job holds goes first,
//! build-offload slots while they pass their byte cap or the disk passes its eviction line, land-offload slots only while the disk does.
//! The lease reaper runs it every 30 s on each build host, a disk-floor admission defer runs it before deciding again, and a disk over the incident
//! line raises one host-disk incident per episode.
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use rustix::fs::{FlockOperation, flock, statvfs};

use crate::platform::disk_budget::{DiskBudget, Share};
use crate::platform::shared_target::allocated_tree;

#[cfg(test)]
mod tests;

pub const EVICT_ABOVE_PERCENT: f64 = 80.0;
pub const INCIDENT_ABOVE_PERCENT: f64 = 85.0;
pub const INCIDENT_CLASS: &str = "host-disk";
const BUILD_OFFLOAD_DISK_SHARE: f64 = 0.15;
const GIB: f64 = (1u64 << 30) as f64;
const EPISODE_FILE: &str = "host-disk.episode";
const COMPANION_DIRS: [&str; 1] = ["beside"];
const COMPANION_FILES: [&str; 2] = ["bundle", "patch"];

/// One cache root and the bytes its slots may hold before the oldest unlocked one goes; `None` evicts only for the disk line.
pub struct Store {
    pub root: PathBuf,
    pub cap_bytes: Option<u64>,
}

pub struct Evicted {
    pub slot: PathBuf,
    pub bytes: u64,
}

/// The root disk as `df` reads it.
#[derive(Clone, Copy)]
pub struct Disk {
    pub used_percent: f64,
    pub free_gib: f64,
    pub size_bytes: u64,
}

struct Slot {
    used: SystemTime,
    bytes: u64,
    path: PathBuf,
    lock: PathBuf,
}

/// The stores of this home in eviction order: build-offload slots are one lane's scratch, land-offload slots the lander's warm checkouts.
pub fn stores(home: &Path, disk: Option<Disk>, budget: &DiskBudget) -> Vec<Store> {
    let share = disk.map(|disk| (disk.size_bytes as f64 * BUILD_OFFLOAD_DISK_SHARE) as u64);
    vec![
        Store { root: home.join(".cache/build-offload"), cap_bytes: budget.bytes(Share::BuildOffload).or(share) },
        Store { root: home.join(".cache/land-offload"), cap_bytes: None },
    ]
}

/// Evicts the least recently used unlocked slot of each store in turn while that store passes its cap or `disk` reads over `line` percent.
pub fn evict(stores: &[Store], line: f64, disk: &mut dyn FnMut() -> Option<Disk>) -> Vec<Evicted> {
    let mut evicted = Vec::new();
    for store in stores {
        let mut slots = slots(&store.root);
        slots.sort_by_key(|slot| slot.used);
        let mut held: u64 = slots.iter().map(|slot| slot.bytes).sum();
        for slot in slots {
            let over_cap = store.cap_bytes.is_some_and(|cap| held > cap);
            if !over_cap && !disk().is_some_and(|disk| disk.used_percent > line) {
                break;
            }
            if remove_unlocked(&slot) {
                held = held.saturating_sub(slot.bytes);
                evicted.push(Evicted { slot: slot.path, bytes: slot.bytes });
            }
        }
    }
    evicted
}

fn slots(root: &Path) -> Vec<Slot> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let lock = root.join(format!("{name}.lock"));
            let used = fs::metadata(&lock).and_then(|metadata| metadata.modified()).ok()?;
            let mut bytes = allocated_tree(&entry.path(), &mut HashSet::new()).map_or(0, |(bytes, _)| bytes);
            for companion in COMPANION_DIRS {
                bytes += allocated_tree(&root.join(format!("{name}.{companion}")), &mut HashSet::new()).map_or(0, |(bytes, _)| bytes);
            }
            Some(Slot { used, bytes, path: entry.path(), lock })
        })
        .collect()
}

fn remove_unlocked(slot: &Slot) -> bool {
    let Ok(lock) = File::open(&slot.lock) else { return false };
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        return false;
    }
    let name = slot.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = slot.path.parent().unwrap_or(Path::new("/"));
    let companions: Vec<PathBuf> = COMPANION_DIRS.iter().map(|suffix| parent.join(format!("{name}.{suffix}"))).collect();
    for tree in std::iter::once(&slot.path).chain(&companions) {
        unregister_within(tree);
    }
    let removed = fs::remove_dir_all(&slot.path).is_ok();
    for directory in &companions {
        let _ = fs::remove_dir_all(directory);
    }
    for suffix in COMPANION_FILES {
        let _ = fs::remove_file(parent.join(format!("{name}.{suffix}")));
    }
    removed
}

/// Unregisters every worktree checked out at `tree` or one level under it from its repository, as `git worktree remove` does, without
/// needing git on PATH: the lease reaper's unit has none, and a slot deleted with its entry left behind refuses the next `worktree add`.
fn unregister_within(tree: &Path) {
    let nested = fs::read_dir(tree).into_iter().flatten().flatten().map(|entry| entry.path());
    for checkout in std::iter::once(tree.to_path_buf()).chain(nested) {
        if let Some(entry) = registration(&checkout)
            && let Err(error) = fs::remove_dir_all(&entry)
        {
            eprintln!("north capacity: could not unregister worktree {} ({}): {error}", checkout.display(), entry.display());
        }
    }
}

/// The repository's `worktrees/NAME` entry for the worktree at `checkout`, only when that entry's own `gitdir` names this checkout.
fn registration(checkout: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(checkout.join(".git")).ok()?;
    let entry = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
    let entry = if entry.is_absolute() { entry } else { checkout.join(entry) };
    (entry.parent()?.file_name()? == "worktrees").then_some(())?;
    let named = PathBuf::from(fs::read_to_string(entry.join("gitdir")).ok()?.trim());
    let named = if named.is_absolute() { named } else { entry.join(named) };
    let same = |left: &Path, right: &Path| left.canonicalize().ok().zip(right.canonicalize().ok()).is_some_and(|(left, right)| left == right);
    same(&named, &checkout.join(".git")).then_some(entry)
}

/// The filesystem holding `path`, as `df` reads it: used over used plus available.
pub fn disk_of(path: &Path) -> Option<Disk> {
    let stats = statvfs(path).ok()?;
    let block = stats.f_frsize;
    let used = stats.f_blocks.saturating_sub(stats.f_bfree).saturating_mul(block);
    let available = stats.f_bavail.saturating_mul(block);
    let total = used + available;
    (total > 0).then(|| Disk { used_percent: used as f64 * 100.0 / total as f64, free_gib: available as f64 / GIB, size_bytes: stats.f_blocks.saturating_mul(block) })
}

/// What a sample does about the incident line: record a new episode under its key, close the one open, or nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Episode {
    Open(String),
    Close,
    Hold,
}

pub fn episode(used_percent: f64, open: Option<&str>, host: &str, now_seconds: u64) -> Episode {
    match (used_percent > INCIDENT_ABOVE_PERCENT, open) {
        (true, None) => Episode::Open(format!("{INCIDENT_CLASS}:{host}@{now_seconds}")),
        (false, Some(_)) => Episode::Close,
        _ => Episode::Hold,
    }
}

fn fixture() -> bool {
    std::env::var_os("AGENT_CAPACITY_DISK_FREE_GIB").is_some()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from)
}

fn evict_here(home: &Path, disk: &mut dyn FnMut() -> Option<Disk>) -> (Vec<Evicted>, Option<Disk>) {
    let first = disk();
    let evicted = evict(&stores(home, first, &DiskBudget::local()), EVICT_ABOVE_PERCENT, disk);
    for gone in &evicted {
        eprintln!("north capacity: evicted offload slot {} ({} MiB), least recently used and unlocked", gone.slot.display(), gone.bytes >> 20);
    }
    (evicted, disk())
}

/// The lease reaper's disk step: evicts, then opens or closes this host's host-disk incident episode.
pub fn tick() {
    if fixture() {
        return;
    }
    let Some(home) = home() else { return };
    let before = disk_of(&home);
    let (evicted, Some(disk)) = evict_here(&home, &mut || disk_of(&home)) else { return };
    let peak = before.map_or(disk.used_percent, |before| before.used_percent.max(disk.used_percent));
    let marker = crate::capacity::agent_load::capacity_root().join(EPISODE_FILE);
    let open = fs::read_to_string(&marker).ok();
    let host = crate::platform::host::name();
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    match episode(peak, open.as_deref().map(str::trim), &host, now) {
        Episode::Open(key) => {
            let detail = format!("{host} root disk at {peak:.0}%, {:.0}% ({:.0} GiB free) after evicting {} unlocked offload slots", disk.used_percent, disk.free_gib, evicted.len());
            if record(&key, &host, &detail) {
                let _ = marker.parent().map(fs::create_dir_all);
                let _ = fs::write(&marker, key);
            }
        }
        Episode::Close => {
            let _ = fs::remove_file(&marker);
        }
        Episode::Hold => {}
    }
}

fn record(key: &str, host: &str, detail: &str) -> bool {
    let north = north_base::platform::north_binary::north_binary();
    let recorded = Command::new(north)
        .args(["incident", "record", "--key", key, "--class", INCIDENT_CLASS, "--noticed-by", "lease-reaper", "--subject", host, "--detail", detail])
        .stdout(Stdio::null())
        .status();
    recorded.is_ok_and(|status| status.success())
}

/// A disk-floor defer's reclaim: evicts offload slots to the line, then runs `rest` (the other reclaim sweeps); the free GiB afterwards,
/// or `None` when neither freed anything.
pub fn reclaim_in(home: &Path, disk: &mut dyn FnMut() -> Option<Disk>, rest: fn() -> Option<f64>) -> Option<f64> {
    let (evicted, after) = evict_here(home, disk);
    rest().or_else(|| (!evicted.is_empty()).then_some(after?.free_gib))
}

pub fn reclaim(rest: fn() -> Option<f64>) -> Option<f64> {
    if fixture() {
        return None;
    }
    home().and_then(|home| reclaim_in(&home, &mut || disk_of(&home), rest))
}
