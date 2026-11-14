mod build_output;
mod capture_batches;
mod cache_cap;
mod ended_lanes;
mod evidence_budget;
mod held;
mod lan_clients;
mod lan_residue;
mod scratch;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod evidence_budget_tests;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use toml_edit::DocumentMut;

use crate::platform::disk_budget::DiskBudget;
use crate::platform::shared_target;

static DEFAULTS: std::sync::LazyLock<&str> = std::sync::LazyLock::new(|| north_base::platform::harness::table(north_base::platform::harness::DEFAULTS));

pub(super) fn setting(name: &str) -> f64 {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    let value = &defaults[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

const USAGE: &str = "usage: north lane reclaim [--dry-run] [--idle-hours H] [--only scratch]";
const DEFAULT_IDLE_HOURS: u64 = 24;
const KEPT_PAIRS: [u32; 2] = [0, 1];

pub(super) struct Inputs {
    tmp: PathBuf,
    proc_root: PathBuf,
    code: PathBuf,
    buck_daemons: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    lan_clients: PathBuf,
    online_clients: PathBuf,
    lan_state: PathBuf,
    evidence: PathBuf,
    capture_queue: PathBuf,
    disk_budget: DiskBudget,
    host: String,
    open_issue_text: fn() -> Result<String, String>,
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
            code: home.join("code"),
            buck_daemons: home.join(".buck/buckd"),
            state: state.clone(),
            cache: home.join(".cache"),
            lan_clients: data.join("wisp/lan/clients"),
            online_clients: data.join("wisp/online"),
            lan_state: state.join("wisp/lan"),
            evidence: state.join("makgora/evidence"),
            capture_queue: state.join("makgora/capture-queue"),
            disk_budget: DiskBudget::local(),
            host: crate::platform::land_queue::local_host(),
            open_issue_text: evidence_budget::open_issue_text,
            uid: rustix::process::getuid().as_raw(),
            now: SystemTime::now(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Scope {
    Everything,
    Scratch,
}

pub(super) struct Policy {
    idle: Duration,
    dry_run: bool,
    scope: Scope,
}

impl Policy {
    pub(super) fn everything(idle: Duration) -> Self {
        Self { idle, dry_run: false, scope: Scope::Everything }
    }
}

pub(super) struct Left {
    pub(super) path: PathBuf,
    pub(super) why: String,
}

#[derive(Default)]
pub(super) struct Reclaimed {
    pub(super) removals: Vec<Removal>,
    pub(super) left: Vec<Left>,
}

pub(super) struct Removal {
    pub(super) path: PathBuf,
    pub(super) reason: String,
    pub(super) bytes: u64,
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(policy) = policy(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let inputs = Inputs::from_env();
    let Reclaimed { removals, left } = match reclaim(&inputs, &policy) {
        Ok(reclaimed) => reclaimed,
        Err(error) => {
            eprintln!("north lane reclaim: {error}");
            return 1;
        }
    };
    let verb = if policy.dry_run { "would remove" } else { "removed" };
    for removal in &removals {
        println!("{verb} {}: {}", removal.path.display(), removal.reason);
    }
    for left in &left {
        println!("left: {} ({})", left.path.display(), left.why);
    }
    println!("{}", summary(&removals, policy.dry_run));
    0
}

fn policy(arguments: &[String]) -> Option<Policy> {
    let mut policy = Policy::everything(Duration::from_secs(DEFAULT_IDLE_HOURS * 3600));
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dry-run" => policy.dry_run = true,
            "--only" if rest.next()? == "scratch" => policy.scope = Scope::Scratch,
            "--idle-hours" => policy.idle = Duration::from_secs(rest.next()?.parse::<u64>().ok()?.checked_mul(3600)?),
            _ => return None,
        }
    }
    Some(policy)
}

pub(super) fn reclaim(inputs: &Inputs, policy: &Policy) -> io::Result<Reclaimed> {
    let held = held::paths(&inputs.proc_root);
    let Reclaimed { mut removals, left } = scratch::sweep(inputs, policy, &held)?;
    if policy.scope == Scope::Scratch {
        return Ok(Reclaimed { removals, left });
    }
    removals.extend(lan_clients::sweep(inputs, policy, &held)?);
    removals.extend(lan_residue::sweep(inputs, policy, &held)?);
    removals.extend(ended_lanes::sweep(inputs, policy, &held));
    removals.extend(build_output::sweep(inputs, policy, &held)?);
    removals.extend(cache_cap::sweep(inputs, policy, &held)?);
    if let Some(budget) = evidence_budget::Budget::of(&inputs.disk_budget) {
        removals.extend(evidence_budget::sweep(inputs, policy, &held, &budget, 0)?);
    }
    removals.extend(capture_batches::sweep(inputs, policy, &held)?);
    Ok(Reclaimed { removals, left })
}

/// `north evidence start RUN`: brings the evidence store under its budget, then creates and prints the run's folder.
pub fn evidence_start(arguments: &[String]) -> u8 {
    let [verb, run] = arguments else {
        eprintln!("usage: north evidence start RUN");
        return 64;
    };
    if verb != "start" || run.is_empty() || run.contains('/') || run == "." || run == ".." {
        eprintln!("usage: north evidence start RUN (RUN is one folder name)");
        return 64;
    }
    let inputs = Inputs::from_env();
    let folder = inputs.evidence.join(run);
    if let Err(error) = fs::create_dir_all(&folder).and_then(|()| fs::File::open(&folder)?.set_modified(SystemTime::now())) {
        eprintln!("north evidence: create {}: {error}", folder.display());
        return 1;
    }
    let policy = Policy::everything(Duration::from_secs(DEFAULT_IDLE_HOURS * 3600));
    if let Some(budget) = evidence_budget::Budget::of(&inputs.disk_budget) {
        match evidence_budget::sweep(&inputs, &policy, &held::paths(&inputs.proc_root), &budget, 0) {
            Ok(removals) => removals.iter().for_each(|removal| eprintln!("removed {}: {}", removal.path.display(), removal.reason)),
            Err(error) => eprintln!("north evidence: budget sweep: {error}"),
        }
    }
    println!("{}", folder.display());
    0
}

/// The capacity host's disk-floor reclaim; a fixed disk reading (AGENT_CAPACITY_DISK_FREE_GIB) is a fixture, so it never reclaims.
pub fn before_disk_defer_here() -> Option<f64> {
    if env::var_os("AGENT_CAPACITY_DISK_FREE_GIB").is_some() {
        return None;
    }
    before_disk_defer(&crate::capacity::agent_load::capacity_root().join("disk-reclaim.stamp"), crate::capacity::enrich::root_disk_free_gib)
}

/// Runs every reclaim sweep once before admission defers on the disk floor, at most once per `disk_reclaim_cooldown_minutes` on this host, and
/// returns the root disk's free GiB afterwards; `None` when it did not run.
fn before_disk_defer(stamp: &Path, free_gib: fn() -> Option<f64>) -> Option<f64> {
    let cooldown = Duration::from_secs_f64(setting("disk_reclaim_cooldown_minutes") * 60.0);
    if fs::metadata(stamp).and_then(|metadata| metadata.modified()).is_ok_and(|at| at.elapsed().unwrap_or_default() < cooldown) {
        return None;
    }
    fs::create_dir_all(stamp.parent()?).ok()?;
    fs::write(stamp, b"").ok()?;
    let policy = Policy::everything(Duration::from_secs(DEFAULT_IDLE_HOURS * 3600));
    match reclaim(&Inputs::from_env(), &policy) {
        Ok(reclaimed) => eprintln!("north capacity: disk under the floor, so reclaimed first: {}", summary(&reclaimed.removals, false)),
        Err(error) => eprintln!("north capacity: disk under the floor; reclaim failed: {error}"),
    }
    free_gib()
}

pub(super) fn idle_for(path: &Path, now: SystemTime) -> io::Result<Duration> {
    Ok(now.duration_since(newest_change(path)?).unwrap_or_default())
}

fn newest_change(path: &Path) -> io::Result<SystemTime> {
    let metadata = fs::symlink_metadata(path)?;
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
        eprintln!("north lane reclaim: remove {}: {error}", path.display());
    }
    removal.is_ok()
}

pub(super) fn hours(duration: Duration) -> u64 {
    duration.as_secs() / 3600
}

pub(super) fn summary(removals: &[Removal], dry_run: bool) -> String {
    let mebibytes = removals.iter().map(|removal| removal.bytes).sum::<u64>() >> 20;
    if dry_run {
        format!("would remove {} entries; {mebibytes} MiB would be freed", removals.len())
    } else {
        format!("removed {} entries; {mebibytes} MiB freed", removals.len())
    }
}

pub(super) fn size(path: &Path) -> u64 {
    shared_target::allocated_tree(path, &mut HashSet::new()).map_or(0, |(bytes, _)| bytes)
}
