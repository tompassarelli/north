//! Keeps a build host's root disk from filling, with wide hysteresis, in the order `harness/disk-ledger.toml` lists the stores the
//! lease reaper trims itself. Passing the high watermark starts one cleanup pass that takes regenerable caches (build-offload slots, the
//! shared cargo targets, wisp window builds, sccache) least recently used first and past their caps, down to the far roomier low
//! watermark, and pages the proxy once with what it took and the ledger's largest stores; the next pass waits for the next crossing.
//! Between passes only caps are kept. Land-offload slots, the lander's reserved slot of each checkout last, are held to their cap on every
//! pass and otherwise go only in a pass that starts under the eviction line, then until the disk is back over the roomier eviction stop
//! line, so a pass never leaves the disk on the line placement refuses at. Within a slot store the least recently used slot whose lock no
//! job holds goes first. The lines are free space, not a percent, so a large disk keeps the same headroom as a small one. The lease
//! reaper starts it every 30 s on each build host as its own unit, and a disk-floor
//! admission defer runs it after the other reclaim sweeps before deciding again.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use rustix::fs::statvfs;

use crate::land_offload::ship::is_lander_slot;
use crate::land_offload::slot_lock;
use crate::platform::disk_budget::DiskBudget;
use crate::platform::disk_ledger::{self, Kind, Ledger};
use crate::platform::shared_target::{self, allocated_tree};
pub use crate::capacity::disk_line::Line;

mod compiler_cache;
#[cfg(test)]
mod tests;

pub const INCIDENT_CLASS: &str = "host-disk";
const BUILD_OFFLOAD_DISK_SHARE: f64 = 0.15;
const GIB: f64 = (1u64 << 30) as f64;
const EPISODE_FILE: &str = "host-disk.episode";
const KEEP_UNIT: &str = "north-disk-keep";
const COMPANION_DIRS: [&str; 1] = ["beside"];
const COMPANION_FILES: [&str; 2] = ["bundle", "patch"];
const ENTRY_IDLE: Duration = Duration::from_secs(20 * 60);
const ALARM_STORES: usize = 3;
const LAND_OFFLOAD: &str = "land_offload";

/// One thing eviction may take from, in the order `stores` lists them, each while the disk passes its own `line`.
pub enum Store {
    /// Offload slots under `root` that `take` admits, the oldest unlocked first, while all of `root`'s slots pass `cap_bytes` or the disk passes `line`.
    Slots { root: PathBuf, cap_bytes: Option<u64>, take: Take, line: Line },
    /// The shared cargo targets under `cache`, idle build output of the least recently built first, to `cap_bytes` and below it by as much as the disk needs.
    CargoTargets { cache: PathBuf, cap_bytes: u64, line: Line },
    /// sccache's local store at `root`, oldest entry first, to `cap_bytes` and below it by as much as the disk needs.
    CompilerCache { root: PathBuf, cap_bytes: u64, line: Line },
    /// The children of `root` none of whose files changed in the last 20 minutes, least recently changed first.
    Entries { root: PathBuf, line: Line },
}

/// The lines a ledger store is trimmed under: regenerable caches at `trim`, warm stores at `evict`.
#[derive(Clone, Copy)]
pub struct Lines {
    pub trim: Line,
    pub evict: Line,
}

impl Lines {
    /// A cleanup pass trims caches to the low watermark; between passes they keep only their caps. Warm stores go only in an evicting
    /// pass, one that started with the host closed to dispatch for disk, and then until the disk is back over `reopened`.
    pub fn pass(cleanup: bool, reopened: Option<Line>) -> Self {
        Self { trim: if cleanup { Line::low_watermark() } else { Line::NEVER }, evict: reopened.unwrap_or(Line::NEVER) }
    }
}

/// The line `budget`'s host is closed to dispatch under on `disk`, and the one an evicting pass reclaims back over.
pub fn closing(budget: &DiskBudget, disk: Option<Disk>) -> (Line, Line) {
    let size_gib = disk.map_or(0.0, |disk| disk.size_bytes as f64 / GIB);
    let bound = budget.dispatch_bound_gib();
    (Line::closed(bound, size_gib), Line::reopened(bound, size_gib))
}

/// Which slots of a root a `Store::Slots` pass may remove.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Take {
    Every,
    AllButLanders,
    Landers,
}

impl Take {
    fn admits(self, slot: &str) -> bool {
        match self {
            Self::Every => true,
            Self::AllButLanders => !is_lander_slot(slot),
            Self::Landers => is_lander_slot(slot),
        }
    }
}

pub struct Evicted {
    pub path: PathBuf,
    pub bytes: u64,
    pub what: &'static str,
}

/// The root disk as `df` reads it.
#[derive(Clone, Copy)]
pub struct Disk {
    pub used_percent: f64,
    pub free_gib: f64,
    pub size_bytes: u64,
}

impl Disk {
    pub fn past(self, line: Line) -> bool {
        line.passed(self.free_gib, self.size_bytes as f64 / GIB)
    }
}

struct Slot {
    used: SystemTime,
    bytes: u64,
    path: PathBuf,
    lock: PathBuf,
}

/// The stores of this home the ledger gives one of the reaper's own trims, in the ledger's order.
pub fn stores(home: &Path, disk: Option<Disk>, budget: &DiskBudget, ledger: &Ledger, lines: Lines) -> Vec<Store> {
    let share = disk.map(|disk| (disk.size_bytes as f64 * BUILD_OFFLOAD_DISK_SHARE) as u64);
    let mut stores = Vec::new();
    for entry in &ledger.entries {
        let place = if entry.path.starts_with('/') { PathBuf::from(&entry.path) } else { home.join(&entry.path) };
        let line = if entry.kind == Kind::Warm { lines.evict } else { lines.trim };
        let cap = budget.named_gib(&entry.name).map(|gib| (gib * GIB) as u64);
        match entry.trim.as_str() {
            "build-slots" => stores.push(Store::Slots { root: place, cap_bytes: cap.or(share), take: Take::Every, line }),
            "cargo-targets" => stores.push(Store::CargoTargets { cache: place.parent().unwrap_or(home).to_path_buf(), cap_bytes: shared_target::cap_bytes(budget), line }),
            "compiler-cache" => stores.push(Store::CompilerCache { root: place, cap_bytes: compiler_cache::cap_bytes(budget), line }),
            "oldest-entries" => stores.push(Store::Entries { root: place, line }),
            "land-slots" => {
                stores.push(Store::Slots { root: place.clone(), cap_bytes: cap, take: Take::AllButLanders, line });
                stores.push(Store::Slots { root: place, cap_bytes: cap, take: Take::Landers, line });
            }
            _ => {}
        }
    }
    stores
}

/// Takes from each store in turn while it passes its cap or `disk` reads past the store's line.
pub fn evict(stores: &[Store], disk: &mut dyn FnMut() -> Option<Disk>) -> Vec<Evicted> {
    let mut evicted = Vec::new();
    for store in stores {
        match store {
            Store::Slots { root, cap_bytes, take, line } => evict_slots(root, *cap_bytes, *take, *line, disk, &mut evicted),
            Store::CargoTargets { cache, cap_bytes, line } => {
                let Some(need) = need(*line, disk) else { continue };
                let trimmed = shared_target::sweep_store(cache, SystemTime::now(), |held| (*cap_bytes).min(held.saturating_sub(need))).unwrap_or_default();
                evicted.extend(trimmed.into_iter().map(|(path, bytes)| Evicted { path, bytes, what: "idle build output of shared cargo target" }));
            }
            Store::CompilerCache { root, cap_bytes, line } => {
                let Some(need) = need(*line, disk) else { continue };
                let bytes = compiler_cache::trim(root, |held| (*cap_bytes).min(held.saturating_sub(need)));
                if bytes > 0 {
                    evicted.push(Evicted { path: root.clone(), bytes, what: "oldest entries of compiler cache" });
                }
            }
            Store::Entries { root, line } => evict_entries(root, *line, disk, &mut evicted),
        }
    }
    evicted
}

fn over(line: Line, disk: &mut dyn FnMut() -> Option<Disk>) -> bool {
    disk().is_some_and(|disk| disk.past(line))
}

/// The bytes the disk must free to come back over `line`, when it reads past it.
fn need(line: Line, disk: &mut dyn FnMut() -> Option<Disk>) -> Option<u64> {
    let disk = disk().filter(|disk| disk.past(line))?;
    Some(((line.floor_gib(disk.size_bytes as f64 / GIB) - disk.free_gib).max(0.0) * GIB) as u64)
}

fn evict_entries(root: &Path, line: Line, disk: &mut dyn FnMut() -> Option<Disk>, evicted: &mut Vec<Evicted>) {
    if !over(line, disk) {
        return;
    }
    let Ok(listing) = fs::read_dir(root) else { return };
    let now = SystemTime::now();
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = listing
        .flatten()
        .filter_map(|entry| allocated_tree(&entry.path(), &mut HashSet::new()).ok().map(|(bytes, touched)| (touched, bytes, entry.path())))
        .filter(|(touched, _, _)| now.duration_since(*touched).is_ok_and(|idle| idle >= ENTRY_IDLE))
        .collect();
    entries.sort_by_key(|(touched, _, _)| *touched);
    for (_, bytes, path) in entries {
        if !over(line, disk) {
            break;
        }
        let removed = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        if removed.is_ok() {
            evicted.push(Evicted { path, bytes, what: "least recently changed entry of a cache" });
        }
    }
}

fn evict_slots(root: &Path, cap_bytes: Option<u64>, take: Take, line: Line, disk: &mut dyn FnMut() -> Option<Disk>, evicted: &mut Vec<Evicted>) {
    if cap_bytes.is_none() && !over(line, disk) {
        return;
    }
    let every = slots(root);
    let mut held: u64 = every.iter().map(|slot| slot.bytes).sum();
    let mut slots: Vec<Slot> = every.into_iter().filter(|slot| slot.path.file_name().is_some_and(|name| take.admits(&name.to_string_lossy()))).collect();
    slots.sort_by_key(|slot| slot.used);
    for slot in slots {
        let over_cap = cap_bytes.is_some_and(|cap| held > cap);
        if !over_cap && !over(line, disk) {
            break;
        }
        if remove_unlocked(&slot) {
            held = held.saturating_sub(slot.bytes);
            evicted.push(Evicted { path: slot.path, bytes: slot.bytes, what: "offload slot" });
        }
    }
}

/// Whether the land-offload slots under `root` take `more` bytes and stay within this host's `land_offload` budget, which the reaper
/// holds them to on every pass, so a warm job never fills a slot the next pass evicts.
pub fn land_slots_take(root: &Path, more: u64) -> bool {
    let Some(cap) = DiskBudget::local().named_gib(LAND_OFFLOAD) else { return true };
    slots(root).iter().map(|slot| slot.bytes).sum::<u64>() + more <= (cap * GIB) as u64
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
    let Some(_held) = slot_lock::take(&slot.lock) else {
        eprintln!("offload cache: {} is held by a run, kept", slot.path.display());
        return false;
    };
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

fn fixture() -> bool {
    std::env::var_os("AGENT_CAPACITY_DISK_FREE_GIB").is_some()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from)
}

/// One eviction pass over this home's stores under `budget`, a cleanup pass when `cleanup` is set or the host reads closed to dispatch
/// for disk, which alone lets the pass take warm stores past their caps, oldest first, until the host is open again with a margin.
fn evict_here(home: &Path, budget: &DiskBudget, disk: &mut dyn FnMut() -> Option<Disk>, cleanup: bool) -> (Vec<Evicted>, Option<Disk>) {
    let first = disk();
    let (closed, reopened) = closing(budget, first);
    let evicting = first.is_some_and(|disk| disk.past(closed));
    let evicted = evict(&stores(home, first, budget, &Ledger::shipped(), Lines::pass(cleanup || evicting, evicting.then_some(reopened))), disk);
    for gone in &evicted {
        eprintln!("north capacity: evicted {} {} ({} MiB), least recently used", gone.what, gone.path.display(), gone.bytes >> 20);
    }
    (evicted, disk())
}

/// The lease reaper's disk step: starts `north capacity disk keep` as its own transient user unit at the lowest CPU and I/O priority.
/// A cleanup pass walks and trims hundreds of thousands of files for a minute or more, past the reaper's 25 s start timeout, and
/// systemd ends every process left in the oneshot reaper's cgroup when it exits; the fixed unit name keeps a second pass from starting
/// beside a running one.
pub fn tend() {
    if fixture() {
        return;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let started = Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect", "--unit", KEEP_UNIT, "--property=Nice=19", "--property=IOSchedulingClass=idle"])
        .arg(format!("--setenv=PATH={}", path.to_string_lossy()))
        .arg("--")
        .arg(north_base::platform::north_binary::north_binary())
        .args(["capacity", "disk", "keep"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(error) = started {
        eprintln!("north capacity: could not start the disk pass: {error}");
    }
}

/// `north capacity disk keep`, the disk pass. A disk passing its high watermark, or the line its host is closed to dispatch under, with
/// no host-disk episode open starts one cleanup pass down to the low watermark, records the episode and pages the proxy once with what the
/// pass took and, when the host stays closed, what holds the space; every pass that reads the host closed reclaims warm stores too. The
/// episode closes once the disk is back over both lines, so the next pass waits for the next crossing. Between passes only caps are kept.
pub fn keep() {
    if fixture() {
        return;
    }
    let Some(home) = home() else { return };
    for seed in north_lanes::lanes::reclaim_dead_seeds(&home.join("code"), Path::new("/proc"), SystemTime::now(), false) {
        eprintln!("north capacity: removed dead lane target seed {} ({} MiB)", seed.path.display(), seed.bytes >> 20);
    }
    let marker = crate::capacity::agent_load::capacity_root().join(EPISODE_FILE);
    let open = marker.exists();
    let Some(before) = disk_of(&home) else { return };
    let budget = DiskBudget::local();
    let (closed, _) = closing(&budget, Some(before));
    let passed = |disk: Disk| disk.past(Line::high_watermark()) || disk.past(closed);
    let starts = !open && passed(before);
    let (evicted, Some(after)) = evict_here(&home, &budget, &mut || disk_of(&home), starts) else { return };
    if starts {
        let host = crate::platform::host::name();
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let key = format!("{INCIDENT_CLASS}:{host}@{now}");
        let largest = disk_ledger::local().map(|account| largest(&account)).unwrap_or_default();
        let detail = alarm(&host, before, after, closed, &evicted, &largest);
        let recorded = record(&key, &host, &detail);
        if page(&detail) || recorded {
            let _ = marker.parent().map(fs::create_dir_all);
            let _ = fs::write(&marker, key);
        }
    } else if open && !passed(after) {
        let _ = fs::remove_file(&marker);
    }
}

/// The ledger's `ALARM_STORES` largest stores as `NAME (KIND, N GiB, trim: HOW)`, then the bytes no store explains.
fn largest(account: &disk_ledger::Account) -> String {
    if account.reflinked {
        return format!("{:.0} GiB used on a filesystem that shares extents, so its stores are not summed", account.used as f64 / GIB);
    }
    let stores: Vec<String> = account.top(ALARM_STORES).iter().map(|row| format!("{} ({}, {:.0} GiB, trim: {})", row.entry.name, row.entry.kind.word(), row.bytes as f64 / GIB, row.entry.trim)).collect();
    format!("{}; {:.0} GiB unaccounted", stores.join(", "), account.unaccounted() as f64 / GIB)
}

/// The one page of a pass that `before` started: what it took, and when `after` still reads under `closed`, that the host stays closed to
/// dispatch with nothing more the rules may reclaim and which stores hold the space.
pub fn alarm(host: &str, before: Disk, after: Disk, closed: Line, evicted: &[Evicted], largest: &str) -> String {
    let freed = evicted.iter().map(|gone| gone.bytes).sum::<u64>() as f64 / GIB;
    let size_gib = after.size_bytes as f64 / GIB;
    let target = Line::low_watermark().floor_gib(size_gib);
    let bound = closed.floor_gib(size_gib);
    let crossed = if before.past(closed) { format!("the {bound:.0} GiB free it is closed to dispatch under") } else { "its high watermark".to_owned() };
    let short = if after.past(closed) {
        format!(", so it stays closed to dispatch under its {bound:.0} GiB disk bound with nothing more its rules may reclaim; what holds the space")
    } else if after.free_gib < target {
        ", short of it with nothing regenerable left, so the rest sits in the stores below; largest stores".to_owned()
    } else {
        "; largest stores".to_owned()
    };
    format!(
        "{host} root disk passed {crossed} at {:.0}% ({:.0} GiB free); one cleanup pass took {} entries ({freed:.0} GiB) toward the low watermark of {target:.0} GiB free and left {:.0} GiB free{short}: {largest}",
        before.used_percent,
        before.free_gib,
        evicted.len(),
        after.free_gib
    )
}

fn page(detail: &str) -> bool {
    let north = north_base::platform::north_binary::north_binary();
    let sent = Command::new(north).args(["fleet", "send", "--from", "service:north-lease-reaper", "proxy", detail]).stdout(Stdio::null()).status();
    sent.is_ok_and(|status| status.success())
}

fn record(key: &str, host: &str, detail: &str) -> bool {
    let north = north_base::platform::north_binary::north_binary();
    let recorded = Command::new(north)
        .args(["ledger", "incident", "record", "--key", key, "--class", INCIDENT_CLASS, "--noticed-by", "lease-reaper", "--subject", host, "--detail", detail])
        .stdout(Stdio::null())
        .status();
    recorded.is_ok_and(|status| status.success())
}

/// A disk-floor defer's reclaim: runs `rest` (the other reclaim sweeps), then evicts offload slots to the line if the disk still passes
/// it; the free GiB afterwards, or `None` when neither freed anything.
pub fn reclaim_in(home: &Path, disk: &mut dyn FnMut() -> Option<Disk>, rest: fn() -> Option<f64>) -> Option<f64> {
    let swept = rest();
    let (evicted, after) = evict_here(home, &DiskBudget::local(), disk, false);
    if evicted.is_empty() { swept } else { after.map(|after| after.free_gib).or(swept) }
}

pub fn reclaim(rest: fn() -> Option<f64>) -> Option<f64> {
    if fixture() {
        return None;
    }
    home().and_then(|home| reclaim_in(&home, &mut || disk_of(&home), rest))
}
