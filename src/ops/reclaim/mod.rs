mod held;
mod lan_clients;
mod scratch;
#[cfg(test)]
mod tests;

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const USAGE: &str = "usage: north reclaim [--dry-run] [--idle-hours H]";
const DEFAULT_IDLE_HOURS: u64 = 24;
const KEPT_PAIRS: [u32; 2] = [0, 1];

pub(super) struct Inputs {
    tmp: PathBuf,
    proc_root: PathBuf,
    lan_clients: PathBuf,
    lan_state: PathBuf,
    uid: u32,
    now: SystemTime,
}

impl Inputs {
    fn from_env() -> Self {
        let set = |name: &str| env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = set("HOME").unwrap_or_default();
        let data = set("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        let state = set("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        Self {
            tmp: PathBuf::from("/tmp"),
            proc_root: PathBuf::from("/proc"),
            lan_clients: data.join("wisp/lan/clients"),
            lan_state: state.join("wisp/lan"),
            uid: rustix::process::getuid().as_raw(),
            now: SystemTime::now(),
        }
    }
}

pub(super) struct Policy {
    idle: Duration,
    dry_run: bool,
}

pub(super) struct Removal {
    pub(super) path: PathBuf,
    pub(super) reason: String,
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let Some(policy) = policy(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let inputs = Inputs::from_env();
    let free_before = free_bytes(&inputs.tmp);
    let removals = match reclaim(&inputs, &policy) {
        Ok(removals) => removals,
        Err(error) => {
            eprintln!("north reclaim: {error}");
            return 1;
        }
    };
    let verb = if policy.dry_run { "would remove" } else { "removed" };
    for removal in &removals {
        println!("{verb} {}: {}", removal.path.display(), removal.reason);
    }
    if policy.dry_run {
        println!("{verb} {} entries", removals.len());
    } else {
        println!("{verb} {} entries; {} MiB freed", removals.len(), free_bytes(&inputs.tmp).saturating_sub(free_before) / (1 << 20));
    }
    0
}

fn policy(arguments: &[String]) -> Option<Policy> {
    let mut policy = Policy { idle: Duration::from_secs(DEFAULT_IDLE_HOURS * 3600), dry_run: false };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dry-run" => policy.dry_run = true,
            "--idle-hours" => policy.idle = Duration::from_secs(rest.next()?.parse::<u64>().ok()?.checked_mul(3600)?),
            _ => return None,
        }
    }
    Some(policy)
}

pub(super) fn reclaim(inputs: &Inputs, policy: &Policy) -> io::Result<Vec<Removal>> {
    let held = held::paths(&inputs.proc_root);
    let mut removals = scratch::sweep(inputs, policy, &held)?;
    removals.extend(lan_clients::sweep(inputs, policy, &held)?);
    Ok(removals)
}

pub(super) fn idle_for(path: &Path, now: SystemTime) -> io::Result<Duration> {
    Ok(now.duration_since(newest_change(path)?).unwrap_or_default())
}

fn newest_change(path: &Path) -> io::Result<SystemTime> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_socket() || metadata.file_type().is_fifo() {
        return Err(io::Error::other(format!("{} is a live endpoint", path.display())));
    }
    let mut newest = metadata.modified()?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            match newest_change(&entry?.path()) {
                Ok(changed) => newest = newest.max(changed),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(newest)
}

pub(super) fn removed(path: &Path) -> bool {
    let removal = fs::symlink_metadata(path).and_then(|metadata| if metadata.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) });
    if let Err(error) = &removal {
        eprintln!("north reclaim: remove {}: {error}", path.display());
    }
    removal.is_ok()
}

pub(super) fn hours(duration: Duration) -> u64 {
    duration.as_secs() / 3600
}

fn free_bytes(path: &Path) -> u64 {
    rustix::fs::statvfs(path).map_or(0, |stats| stats.f_bavail.saturating_mul(stats.f_frsize))
}
