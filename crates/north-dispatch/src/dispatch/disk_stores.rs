//! What the harness keeps on this dispatch host's root disk, each store sized by a walk bounded in time so the read finishes, and the disk keep the harness already has for its own stores, run here because this host imports no lease reaper.
use north_base::platform::disk_ledger::{self, Ledger};
use north_base::platform::process::run_within;
use north_fleet_supervisor::fleet::records::{ARCHIVE, PLACED, read_phased};
use north_fleet_supervisor::fleet::session_lifecycle::{Phase, Phases};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WALK: Duration = Duration::from_secs(60);
const KEEP: Duration = Duration::from_secs(900);
const CACHES: [&str; 7] = ["north_cache", "mutant_cargo", "mutant_sweeps", "build_offload", "land_offload", "sccache", "cargo_target"];
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Default)]
struct Size {
    bytes: u64,
    entries: u64,
    stopped: bool,
}

fn walk(path: &Path, deadline: Instant, device: Option<u64>, size: &mut Size) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if device.is_some_and(|device| device != meta.dev()) {
        return;
    }
    size.bytes += meta.blocks() * 512;
    size.entries += 1;
    if !meta.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        if Instant::now() > deadline {
            size.stopped = true;
            return;
        }
        walk(&entry.path(), deadline, Some(meta.dev()), size);
    }
}

fn sized(paths: &[PathBuf]) -> Size {
    let deadline = Instant::now() + WALK;
    let mut size = Size::default();
    for path in paths {
        walk(path, deadline, None, &mut size);
    }
    size
}

fn line(name: &str, size: &Size, at: &str) -> (u64, String) {
    let stopped = if size.stopped { format!(", walk stopped at {} s so at least this", WALK.as_secs()) } else { String::new() };
    (size.bytes, format!("{:>8.2} GiB in {} entries  {name} ({at}{stopped})", size.bytes as f64 / GIB, size.entries))
}

fn children(directory: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| keep(path)).collect()
}

fn sessions(fleet: &Path) -> Vec<(String, Vec<PathBuf>)> {
    let mut phases = Phases::reading(fleet);
    let (mut live, mut parked, mut ended, mut unread) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for directory in children(fleet, |path| path.join("session.json").is_file()) {
        match read_phased(phases.as_mut(), &directory).ok().and_then(|record| record.phase()) {
            Some(Phase::Live) => live.push(directory),
            Some(Phase::Parked) => parked.push(directory),
            Some(phase) if phase.ended() => ended.push(directory),
            _ => unread.push(directory),
        }
    }
    [("live", live), ("parked", parked), ("ended", ended), ("adopted or unphased", unread)].into_iter().map(|(class, paths)| (class.to_owned(), paths)).collect()
}

/// The harness's stores on this host, largest first: the runtimes, the fleet's archive, its session directories by state with their counts, its placed mirrors, the land-queue logs, the graph and ledger databases, the harness cache stores and the provider profile transcripts.
pub(super) fn read(home: &Path) -> Vec<String> {
    let state = home.join(".local/state/north");
    let fleet = state.join("fleet");
    let ledger = Ledger::shipped();
    let mut stores: Vec<(String, Vec<PathBuf>, String)> = vec![
        ("runtimes".into(), vec![state.join("runtimes")], state.join("runtimes").display().to_string()),
        ("fleet archive".into(), vec![fleet.join(ARCHIVE)], fleet.join(ARCHIVE).display().to_string()),
        ("fleet placed mirrors".into(), vec![fleet.join(PLACED)], fleet.join(PLACED).display().to_string()),
        ("land-queue logs".into(), children(&north_base::platform::land_queue::queues(&home.join(".local/state")), |_| true).into_iter().map(|queue| queue.join("logs")).collect(), format!("{}/*/logs", north_base::platform::land_queue::queues(&home.join(".local/state")).display())),
        ("graph database".into(), children(&state, |path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("graph.db"))), state.join("graph.db*").display().to_string()),
        ("ledger database".into(), children(&state, |path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("ledger.db"))), state.join("ledger.db*").display().to_string()),
        ("provider profile transcripts".into(), children(&state.join("profiles"), |_| true).into_iter().map(|profile| profile.join("projects")).collect(), state.join("profiles/*/projects").display().to_string()),
    ];
    for (class, paths) in sessions(&fleet) {
        stores.push((format!("{} {class} fleet sessions", paths.len()), paths, format!("{}/NAME", fleet.display())));
    }
    for name in CACHES {
        if let Some(entry) = ledger.entry(name) {
            stores.push((format!("cache {name}"), entry.resolve(home), home.join(&entry.path).display().to_string()));
        }
    }
    let mut rows: Vec<(u64, String)> = std::thread::scope(|scope| {
        let handles: Vec<_> = stores.iter().map(|(name, paths, at)| scope.spawn(move || line(name, &sized(paths), at))).collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
    });
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.into_iter().map(|(_, text)| text).collect()
}

/// Runs `north capacity disk keep` here within `KEEP` (runtimes past the newest ten that no link, process or lease needs, land-queue logs over seven days, then the cache stores of the disk ledger), and says what it freed and each thing it removed.
pub(super) fn keep(home: &Path) -> Vec<String> {
    let before = disk_ledger::used_bytes(home);
    let north = north_base::platform::north_binary::north_binary();
    let output = run_within(&north.to_string_lossy(), &["capacity", "disk", "keep"], KEEP);
    let after = disk_ledger::used_bytes(home);
    let freed = match (before, after) {
        (Some(before), Some(after)) => format!("{:.2} GiB", before.saturating_sub(after) as f64 / GIB),
        _ => "an unread amount".to_owned(),
    };
    let ran = match &output {
        Some(output) if output.status.success() => "ran".to_owned(),
        Some(output) => format!("failed ({})", output.status),
        None => format!("did not finish within {} s", KEEP.as_secs()),
    };
    let removed = output.map(|output| String::from_utf8_lossy(&output.stderr).lines().map(|line| format!("keep: {}", line.trim())).collect::<Vec<_>>()).unwrap_or_default();
    std::iter::once(format!("keep: the harness's disk keep {ran} and freed {freed}")).chain(removed).collect()
}
