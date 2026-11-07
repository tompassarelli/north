use super::bounded;
use super::cargo_build::{self, BuildLog};
use super::host_facts;
use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, BUILT_HERE, Layout, PROGRAMS, TREE};
use super::lifecycle::{self, Promoting, Promotion};
use super::live;
use super::peers;
use super::pull;
use super::remote;
use super::reuse;
use super::store;
use crate::capacity::game_reserve::Confinement;
use crate::land_offload;
use crate::pacing::placement::gate;
use crate::platform::{host, process};
use rustix::fs::{FlockOperation, flock};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FETCH_LIMIT: Duration = Duration::from_secs(120);
const PINNED_LOCK_WAIT: Duration = Duration::from_secs(10 * 60);
const ROLLBACK_LOCK_WAIT: Duration = Duration::from_secs(2 * 60);

const PASSES: usize = 5;
const ABANDONED_COPY: Duration = Duration::from_secs(60 * 60);
const UPDATE_LOCK: &str = "runtime-update.lock";
pub(super) const DEPLOY_LOCK: &str = "runtime-deploy.lock";

pub(super) mod hold;
mod sync;

use hold::Verdict;
use sync::{republish_if_unpublished, swap, sync};
pub(super) use sync::alert_proxy;

enum Pass {
    Settled,
    Promoted,
}

pub(super) fn update(layout: &Layout) -> Result<(), Failure> {
    recorded(layout, lock(layout, FlockOperation::NonBlockingLockExclusive), follow_main)
}

pub(super) fn update_after_landing(layout: &Layout) -> Result<(), Failure> {
    recorded(layout, lock_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime promote").map(Some), |layout| pass(layout).map(|_| ()))
}

fn recorded(layout: &Layout, lock: Result<Option<File>, Failure>, update: impl FnOnce(&Layout) -> Result<(), Failure>) -> Result<(), Failure> {
    let updated = lock.and_then(|lock| match lock {
        Some(_lock) => update(layout),
        None => {
            println!("north runtime update already running");
            Ok(())
        }
    });
    host_facts::record_if_stale(layout);
    updated
}

fn follow_main(layout: &Layout) -> Result<(), Failure> {
    for _ in 0..PASSES {
        match pass(layout) {
            Ok(Pass::Promoted) => continue,
            finished => return finished.map(|_| ()),
        }
    }
    Err(failed(DEFERRED, "origin/main kept moving past each build; retrying on the next tick"))
}

pub(super) fn update_to(layout: &Layout, target: &str) -> Result<(), Failure> {
    if !land_offload::accepts_peer_runtimes_here() {
        return Err(failed(1, format!("this host builds its own runtime and does not promote {} shipped from another host", layout::short(target))));
    }
    let _lock = lock_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime update --sha")?;
    let checkout = checkout();
    fetch(&checkout)?;
    let previous = fs::read_link(layout.current()).ok();
    if previous.as_deref() == Some(layout::link(target).as_path()) {
        println!("north runtime current: {}", layout::short(target));
        return Ok(());
    }
    if let Some(newer) = previous.as_deref().and_then(layout::linked_sha).filter(|current| ancestor(&checkout, target, current)) {
        println!("north runtime: not moving {} back from {} to {}", host::name(), layout::short(newer), layout::short(target));
        return Ok(());
    }
    if !usable(&layout.runtime(target)) {
        return Err(failed(1, format!("{} was not shipped to this host; runtime unchanged", layout::short(target))));
    }
    advance(layout, &checkout, target, previous.as_deref()).map(|_| ())
}

pub(super) fn roll_back(layout: &Layout, failed_sha: &str) -> Result<String, Failure> {
    let _lock = lock_waiting(layout, remote::limit(ROLLBACK_LOCK_WAIT), "north runtime rollback")?;
    let current = fs::read_link(layout.current()).ok();
    if current.as_deref() != Some(layout::link(failed_sha).as_path()) {
        return Err(failed(1, format!("current is not {}; nothing to roll back", layout::short(failed_sha))));
    }
    if !lifecycle::confirm_current(layout, failed_sha)? {
        return Err(failed(1, format!("{} never finished its promotion; nothing to roll back", layout::short(failed_sha))));
    }
    let Some(previous) = lifecycle::previous(layout, failed_sha, |sha| layout::has_programs(&layout.runtime(sha)))? else {
        return Err(failed(1, format!("no previous runtime to roll {} back to", layout::short(failed_sha))));
    };
    swap(layout, &layout::link(&previous))?;
    live::note_swap(layout, &layout::link(&previous));
    lifecycle::move_to::<Promoting>(layout, failed_sha, Promotion::RolledBack, "rolled back on this host after promotion")?;
    lifecycle::move_to::<Promoting>(layout, &previous, Promotion::Live, &format!("rolled back to after {} failed", layout::short(failed_sha)))?;
    println!("north runtime rolled back to {}", layout::short(&previous));
    Ok(layout::short(&previous).to_owned())
}

pub(super) fn lock_waiting(layout: &Layout, limit: Duration, waiter: &str) -> Result<File, Failure> {
    named_lock_waiting(layout, UPDATE_LOCK, limit, waiter)
}

pub(super) fn named_lock_waiting(layout: &Layout, name: &str, limit: Duration, waiter: &str) -> Result<File, Failure> {
    let started = Instant::now();
    let mut announced = false;
    loop {
        if let Some(held) = named_lock(layout, name, FlockOperation::NonBlockingLockExclusive)? {
            return Ok(held);
        }
        if !announced {
            println!("{waiter}: waiting up to {}s for {name}, held by another north runtime process", limit.as_secs());
            announced = true;
        }
        if started.elapsed() >= limit {
            return Err(failed(1, format!("{name} stayed held by another north runtime process for {}s", limit.as_secs())));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn pass(layout: &Layout) -> Result<Pass, Failure> {
    let checkout = checkout();
    let target = main_commit(&checkout)?;
    let previous = fs::read_link(layout.current()).ok();
    let linked = previous.as_deref() == Some(layout::link(&target).as_path());
    if linked && lifecycle::confirm_current(layout, &target)? {
        println!("north runtime current: {}", layout::short(&target));
        republish_if_unpublished(layout, &target);
        return Ok(Pass::Settled);
    }
    let fallback = match linked {
        true => lifecycle::previous(layout, &target, |sha| layout::has_programs(&layout.runtime(sha)))?.map(|sha| layout::link(&sha)),
        false => previous,
    };
    match advance(layout, &checkout, &target, fallback.as_deref()) {
        Err(deferred) if deferred.code == DEFERRED => catch_up(layout, &checkout, &target, fallback.as_deref())?.ok_or(deferred),
        advanced => advanced,
    }
}

fn advance(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<Pass, Failure> {
    let short = layout::short(target);
    let was = previous.and_then(layout::linked_sha);
    if let Some(was) = was {
        lifecycle::confirm_current(layout, was)?;
    }
    let verdict = hold::verdict(layout, target, usable(&layout.runtime(target)))?;
    if let Some(notice) = verdict.notice(short, was.unwrap_or("none")) {
        println!("{notice}");
    }
    if let Verdict::Held(_) = verdict {
        return Ok(Pass::Settled);
    }
    let promoted = promote(layout, checkout, target, previous);
    if let Err((stopped, failure)) = &promoted {
        let stopped = if failure.code == DEFERRED { Promotion::Unfinished } else { *stopped };
        lifecycle::move_to::<Promoting>(layout, target, stopped, &failure.message)?;
    }
    let synced = promoted.map_err(|(_, failure)| failure)?;
    lifecycle::went_live(layout, target, was)?;
    live::note_swap(layout, &layout::link(target));
    println!("{synced}north runtime promoted: {short} (was {})", was.unwrap_or("none"));
    host_facts::record_after_promotion(layout, target);
    collect(layout, target, previous).map_err(|error| failed(1, format!("collecting old runtimes: {error}")))?;
    Ok(Pass::Promoted)
}

fn catch_up(layout: &Layout, checkout: &Path, main: &str, previous: Option<&Path>) -> Result<Option<Pass>, Failure> {
    let current = previous.and_then(layout::linked_sha);
    let ahead = |sha: &str| sha != main && ancestor(checkout, sha, main) && current.is_none_or(|current| sha != current && !ancestor(checkout, sha, current));
    let promoted = peers::promoted().into_iter().filter(|peer| ahead(&peer.sha)).collect();
    let Some(newest) = peers::newest(promoted, |later, earlier| ancestor(checkout, earlier, later)) else {
        return Ok(None);
    };
    let runtime = layout.runtime(&newest.sha);
    if !usable(&runtime) && !pull::pull_from(&newest.host, &newest.sha, &runtime) {
        return Ok(None);
    }
    println!("north runtime: {} is not built anywhere yet; catching up to {}, promoted on {}", layout::short(main), layout::short(&newest.sha), newest.host);
    advance(layout, checkout, &newest.sha, previous)?;
    Ok(Some(Pass::Settled))
}

fn ancestor(checkout: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(checkout), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
}

fn promote(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<String, (Promotion, Failure)> {
    let unfinished = |failure| (Promotion::Unfinished, failure);
    let building = !usable(&layout.runtime(target));
    if building {
        lifecycle::move_to::<Promoting>(layout, target, Promotion::Building, "pulling a peer's build or building here").map_err(unfinished)?;
    }
    let runtime = built(layout, checkout, target).map_err(|failure| (Promotion::BuildFailed, failure))?;
    let cause = if building { "built; swapping current to it and checking it" } else { "built elsewhere and shipped; swapping current to it and checking it" };
    lifecycle::move_to::<Promoting>(layout, target, Promotion::Installing, cause).map_err(unfinished)?;
    install(layout, checkout, target, &runtime, previous).map_err(|failure| (Promotion::InstallFailed, failure))
}

fn install(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, previous: Option<&Path>) -> Result<String, Failure> {
    if !runtime.join(TREE).is_dir() {
        export_tree(checkout, target, runtime)?;
    }
    super::entry_links::link(runtime)?;
    store::root(runtime)?;
    std::thread::scope(|scope| {
        scope.spawn(|| super::deploy::stage_for_lander(layout, target, runtime));
        sync(layout, previous, target)
    })
}

pub(super) fn lock(layout: &Layout, operation: FlockOperation) -> Result<Option<File>, Failure> {
    named_lock(layout, UPDATE_LOCK, operation)
}

fn named_lock(layout: &Layout, name: &str, operation: FlockOperation) -> Result<Option<File>, Failure> {
    fs::create_dir_all(layout.runtimes())?;
    let lock = File::create(layout.state.join(name))?;
    Ok(flock(&lock, operation).is_ok().then_some(lock))
}

pub(super) fn built_main(layout: &Layout, checkout: &Path) -> Result<(String, PathBuf), Failure> {
    let target = main_commit(checkout)?;
    let wanted = layout.runtime(&target);
    if !usable(&wanted) && !reused(layout, checkout, &target, &wanted) {
        println!("north runtime deploy: building {} (up to 25 min under a heavy capacity lease)", layout::short(&target));
    }
    let runtime = built(layout, checkout, &target)?;
    Ok((target, runtime))
}

pub(super) fn main_commit(checkout: &Path) -> Result<String, Failure> {
    fetch(checkout)?;
    git(checkout, &["rev-parse", "--verify", "origin/main^{commit}"])
}

fn built(layout: &Layout, checkout: &Path, target: &str) -> Result<PathBuf, Failure> {
    let runtime = layout.runtime(target);
    if !usable(&runtime) && !reused(layout, checkout, target, &runtime) {
        build(layout, checkout, target, &runtime)?;
    }
    Ok(runtime)
}

fn usable(runtime: &Path) -> bool {
    layout::usable(runtime, land_offload::settles_fleet_here())
}

fn reused(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> bool {
    reuse::reuse(layout, checkout, target, runtime, land_offload::settles_fleet_here())
}

pub(super) fn checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| layout::home().join("code/north/main"))
}

fn git<S: AsRef<OsStr>>(checkout: &Path, args: &[S]) -> Result<String, Failure> {
    let output = process::run("git", Some(checkout), args)?;
    if !output.status.success() {
        let shown = args.iter().map(|arg| arg.as_ref().to_string_lossy()).collect::<Vec<_>>();
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(failed(1, format!("git {} failed: {}", shown.join(" "), stderr.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

struct Lane<'a> {
    checkout: &'a Path,
    path: PathBuf,
}

impl<'a> Lane<'a> {
    fn add(checkout: &'a Path, target: &str) -> Result<Self, Failure> {
        let path = layout::home().join("code/north/worktrees/runtime-build");
        let lane = Self { checkout, path };
        lane.remove();
        git(checkout, &[OsStr::new("worktree"), "add".as_ref(), "-q".as_ref(), "--detach".as_ref(), lane.path.as_os_str(), target.as_ref()])?;
        Ok(lane)
    }

    fn remove(&self) {
        let _ = git(self.checkout, &[OsStr::new("worktree"), "remove".as_ref(), "--force".as_ref(), self.path.as_os_str()]);
        let _ = git(self.checkout, &["worktree", "prune"]);
    }
}

impl Drop for Lane<'_> {
    fn drop(&mut self) {
        self.remove();
    }
}

enum Placement {
    Elsewhere(Box<gate::Machine>),
    Here { refused: Option<String> },
    Confined(Confinement),
    Held,
}

fn placement(reserved: bool, builds_only_here: bool, confinement: impl FnOnce() -> Option<Confinement>, other_host: impl FnOnce() -> Result<gate::Machine, String>) -> Placement {
    if !reserved {
        return Placement::Here { refused: None };
    }
    if builds_only_here {
        return confinement().map_or(Placement::Held, Placement::Confined);
    }
    match other_host() {
        Ok(machine) => Placement::Elsewhere(Box::new(machine)),
        Err(why) => Placement::Here { refused: Some(why) },
    }
}

fn build(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    if pull::pull(target, runtime) {
        return Ok(());
    }
    let other_host = || match land_offload::accepts_peer_runtimes_here() {
        true => land_offload::host_with_room(),
        false => Err("it never takes a runtime built on another host".to_owned()),
    };
    let mut confined = None;
    match placement(land_offload::in_game_reserve(), land_offload::settles_fleet_here(), crate::capacity::game_reserve::confinement, other_host) {
        Placement::Elsewhere(machine) => return remote::build(&machine, target, runtime),
        Placement::Confined(confinement) => {
            println!("north runtime: this host is in game-reserve; building {} on the agent CPUs {} at idle priority", layout::short(target), confinement.cpus);
            confined = Some(confinement);
        }
        Placement::Held => return Err(failed(DEFERRED, format!("this host builds its own runtime and is in game-reserve with the agent CPUs not enforced; {} waits for the reserve to end and the live runtime stays", layout::short(target)))),
        Placement::Here { refused: Some(why) } => println!("north runtime: this host is in game-reserve and {why}; building {} here under the runtime class admission", layout::short(target)),
        Placement::Here { refused: None } => {}
    }
    if !land_offload::builds_runtime_here() {
        return Err(failed(DEFERRED, format!("no other host has built {} yet and this host does not build runtimes (builds_runtime = false); retrying on the next tick", layout::short(target))));
    }
    build_here(layout, checkout, target, runtime, confined.as_ref())
}

fn build_here(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, confined: Option<&Confinement>) -> Result<(), Failure> {
    let lane = Lane::add(checkout, target)?;
    let built = layout::home().join(".cache/cargo-target/north");
    let mut build = Command::new(north_base::platform::north_binary::north_binary());
    build
        .args(["capacity", "host", "run", "--class", "runtime", "--owner", "north-runtime", "--timeout-seconds", "1500", "--"])
        .args(cargo_build::cargo_words(confined))
        .current_dir(&lane.path)
        .env("CARGO_TARGET_DIR", &built)
        .env("CARGO_PROFILE_RELEASE_INCREMENTAL", "true");
    let log = BuildLog::open(layout, target)?;
    let mut attempt = || -> Result<_, Failure> {
        let wall = std::time::SystemTime::now();
        log.attach(&mut build)?;
        let status = build.status()?;
        Ok((status, cargo_build::cargo_ran(&built, wall)))
    };
    let (mut status, mut ran) = attempt()?;
    if let Some(archives) = log.stale_archives().filter(|_| !status.success() && ran) {
        println!(
            "north runtime: build of {} failed to link against stale release incremental output (build log: {}); clearing {} and {} stale rlibs and building once more",
            layout::short(target),
            log.path().display(),
            built.join("release/incremental").display(),
            archives.len()
        );
        cargo_build::recompile_from_scratch(&built, &archives)?;
        (status, ran) = attempt()?;
    }
    if !status.success() {
        return Err(cargo_build::build_failure(target, status, ran, &log));
    }
    log.discard();
    let release = built.join("release");
    if !layout::has_programs(&release) {
        return Err(failed(DEFERRED, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        layout::install(&release.join(program), &runtime.join(program))?;
    }
    fs::write(runtime.join(BUILT_HERE), layout::short(target))?;
    Ok(())
}

fn export_tree(checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    let staging = runtime.join(format!("{TREE}.partial"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["archive", "--format=tar", target])
        .stdout(Stdio::piped())
        .spawn()?;
    let piped = archive.stdout.take().ok_or_else(|| failed(1, "git archive gave no output"))?;
    let extracted = Command::new("tar").arg("-x").arg("-C").arg(&staging).stdin(piped).status()?;
    let archived = archive.wait()?;
    if !(archived.success() && extracted.success()) {
        return Err(failed(1, format!("tree export of {target} failed: git archive {archived}, tar {extracted}")));
    }
    fs::rename(&staging, runtime.join(TREE))?;
    Ok(())
}

pub(crate) fn collect(layout: &Layout, target: &str, previous: Option<&Path>) -> std::io::Result<()> {
    reap_unfinished_copies(&layout.runtimes(), ABANDONED_COPY)?;
    let mut listed = Vec::new();
    for entry in fs::read_dir(layout.runtimes())? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            listed.push((entry.metadata()?.modified()?, entry.file_name().to_string_lossy().into_owned()));
        }
    }
    listed.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let names = listed.into_iter().map(|(_, name)| name).collect::<Vec<_>>();
    let mut keep = vec![target];
    keep.extend(previous.and_then(layout::linked_sha));
    let lander = fs::read_link(layout.lander()).ok();
    keep.extend(lander.as_deref().and_then(layout::linked_sha));
    let running = layout::running(Path::new("/proc"), &layout.runtimes());
    keep.extend(running.iter().map(String::as_str));
    let current = fs::canonicalize(layout.current()).ok();
    for name in layout::doomed(&names, &keep) {
        let doomed = layout.runtime(name);
        if fs::canonicalize(&doomed).ok() == current {
            continue;
        }
        let Some(_unleased) = layout::unleased(&doomed)? else {
            continue;
        };
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), doomed.as_os_str()])?;
        fs::remove_dir_all(&doomed)?;
    }
    Ok(())
}

/// A copy shipped over a slow link stages in `SHA.staging` and moves into place only whole, so one that stopped part-way leaves its staging directory behind; one untouched for `idle` has no copy writing to it.
fn reap_unfinished_copies(runtimes: &Path, idle: Duration) -> std::io::Result<()> {
    for entry in fs::read_dir(runtimes)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let unfinished = [".staging", ".old"].iter().any(|suffix| name.strip_suffix(suffix).is_some_and(layout::is_sha));
        if !unfinished || !entry.file_type()?.is_dir() || entry.metadata()?.modified()?.elapsed().unwrap_or_default() < idle {
            continue;
        }
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), entry.path().as_os_str()])?;
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

fn fetch(checkout: &Path) -> Result<(), Failure> {
    let mut command = Command::new("git");
    command.arg("-C").arg(checkout).args(["fetch", "-q"]);
    let status = bounded::status(&mut command, FETCH_LIMIT).map_err(|error| failed(1, format!("git fetch failed: {error}")))?;
    match status.success() {
        true => Ok(()),
        false => Err(failed(code(status), format!("git fetch failed: {status}"))),
    }
}

#[cfg(test)]
mod policy_hold_tests;
#[cfg(test)]
mod tests;
