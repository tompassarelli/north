//! Keeps a build host's land-check slots warm between landings. While at most `BUSY_SLOTS` land checks hold slots here, the lease reaper starts one warm job: it
//! takes the unlocked slot furthest behind its repository's origin/main, never the lander's reserved one, checks it out at that head and
//! runs the repository's `landing.warm` command there at the lowest CPU priority, so the next check on any slot, whichever package set it
//! builds and whether or not the last after-landing run passed, compiles only what changed since.
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

use super::ship::{SLOTS, is_lander_slot, slot_ports};

const LOCK: &str = "warm.lock";
const LOG: &str = "warm.log";
const SETTING: &str = "landing.warm";
const DEADLINE: Duration = Duration::from_secs(1800);
const POLL: Duration = Duration::from_millis(500);
const BUSY_SLOTS: usize = 2;

struct Slot {
    name: String,
    checkout: String,
    repo: PathBuf,
    at: String,
}

fn root() -> Option<PathBuf> {
    env::var_os("NORTH_OFFLOAD_ROOT")
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").filter(|home| !home.is_empty()).map(|home| PathBuf::from(home).join(".cache/land-offload")))
}

fn held(lock: &Path) -> bool {
    File::open(lock).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockShared).is_err())
}

fn take(lock: &Path) -> Option<File> {
    let file = OpenOptions::new().create(true).append(true).open(lock).ok()?;
    flock(&file, FlockOperation::NonBlockingLockExclusive).ok().map(|()| file)
}

fn too_busy(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else { return false };
    let held_slots = entries.flatten().map(|entry| entry.path()).filter(|path| {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        name.ends_with(".lock") && name != LOCK && !name.ends_with(".background.lock") && held(path)
    });
    held_slots.count() > BUSY_SLOTS
}

/// The lease reaper's step: starts a detached warm job when none is in flight and at most `BUSY_SLOTS` land checks hold slots here.
pub fn tend() {
    let Some(root) = root().filter(|root| root.is_dir()) else { return };
    if held(&root.join(LOCK)) || too_busy(&root) {
        return;
    }
    let Ok(log) = OpenOptions::new().create(true).append(true).open(root.join(LOG)) else { return };
    let Ok(errors) = log.try_clone() else { return };
    let started = Command::new(north_base::platform::north_binary::north_binary())
        .args(["capacity", "offload", "warm"])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .process_group(0)
        .spawn();
    if let Err(error) = started {
        eprintln!("north capacity: could not start the slot warmer: {error}");
    }
}

/// `north capacity offload warm`: warms one slot, or does nothing when another warm job, a busy host or no stale slot stands in the way.
pub fn run() -> u8 {
    let Some(root) = root() else { return 0 };
    let Some(_warming) = take(&root.join(LOCK)) else { return 0 };
    if too_busy(&root) {
        return 0;
    }
    let mut heads = BTreeMap::new();
    let mut stale: Vec<(usize, Slot, String)> = slots(&root)
        .into_iter()
        .filter_map(|slot| {
            let head = heads.entry(slot.repo.clone()).or_insert_with(|| main_head(&slot.repo)).clone()?;
            (slot.at != head).then(|| (changed_files(&slot.repo, &slot.at, &head), slot, head))
        })
        .collect();
    stale.sort_by_key(|(distance, _, _)| std::cmp::Reverse(*distance));
    for (_, slot, head) in stale {
        let Some(_held) = take(&root.join(format!("{}.lock", slot.name))) else { continue };
        return warm(&root, &slot, &head);
    }
    0
}

fn slots(root: &Path) -> Vec<Slot> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path();
            if is_lander_slot(&name) || !path.join("target").is_dir() {
                return None;
            }
            let gitdir = fs::read_to_string(path.join(".git")).ok()?;
            let repo = PathBuf::from(gitdir.strip_prefix("gitdir:")?.trim()).ancestors().nth(3)?.to_path_buf();
            let checkout = (1..SLOTS).find_map(|index| name.strip_suffix(&format!("-{index}"))).unwrap_or(&name).to_owned();
            let at = git(&path, &["rev-parse", "HEAD"])?;
            Some(Slot { name, checkout, repo, at })
        })
        .collect()
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|text| !text.is_empty())
}

fn main_head(repo: &Path) -> Option<String> {
    let _ = git(repo, &["fetch", "-q", "--no-auto-maintenance", "origin", "main"]);
    git(repo, &["rev-parse", "refs/remotes/origin/main"])
}

fn changed_files(repo: &Path, at: &str, head: &str) -> usize {
    git(repo, &["diff", "--name-only", at, head]).map_or(usize::MAX, |files| files.lines().count())
}

fn warm(root: &Path, slot: &Slot, head: &str) -> u8 {
    let Some(command) = git(&slot.repo, &["config", "--blob", "refs/remotes/origin/main:.safe-push", "--get", SETTING]) else { return 0 };
    let path = root.join(&slot.name);
    if git(&path, &["checkout", "-q", "--detach", "-f", head]).is_none() && git(&path, &["rev-parse", "HEAD"]).as_deref() != Some(head) {
        eprintln!("north capacity: could not check {} out at {head}", path.display());
        return 1;
    }
    let _ = git(&path, &["clean", "-ffdxq", "-e", "/target"]);
    let holder = root.join(format!("{}.holder", slot.name));
    let _ = fs::write(&holder, format!("{} warming\n", &head[..head.len().min(12)]));
    let port = slot_ports(&slot.checkout).into_iter().find(|(name, _)| *name == slot.name).map(|(_, port)| port.to_string());
    let share = std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).max(1)).to_string();
    let mut child = Command::new("nice");
    child.args(["-n", "19"]).arg(north_base::platform::north_binary::north_binary()).args(["dev-shell", "run", "--dir"]).arg(&path).args(["--", "bash", "-euo", "pipefail", "-c", &command]);
    child.current_dir(&path).env("SCCACHE_BASEDIRS", &path).env("CARGO_BUILD_JOBS", &share).stdin(Stdio::null()).process_group(0);
    if let Some(port) = port {
        child.env("SCCACHE_SERVER_PORT", port);
    }
    let started = Instant::now();
    let status = match child.spawn() {
        Ok(mut running) => loop {
            if let Ok(Some(status)) = running.try_wait() {
                break status.code().map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
            }
            if started.elapsed() > DEADLINE {
                if let Some(pid) = rustix::process::Pid::from_raw(running.id().cast_signed()) {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
                }
                let _ = running.wait();
                break format!("killed after {} s", DEADLINE.as_secs());
            }
            std::thread::sleep(POLL);
        },
        Err(error) => format!("not started: {error}"),
    };
    let _ = fs::remove_file(&holder);
    eprintln!("north capacity: warmed {} at {} in {} s ({status})", slot.name, &head[..head.len().min(12)], started.elapsed().as_secs());
    0
}
