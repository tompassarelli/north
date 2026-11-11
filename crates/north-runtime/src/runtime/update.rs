use super::canary;
use super::bounded;
use super::host_facts;
use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, Layout, TREE};
use super::lifecycle::{self, Promoting, Promotion};
use super::live;
use super::peers;
use super::pin;
use super::pull;
use super::probe::Shell;
use super::remote;
use super::reuse;
use super::store;
use crate::land_offload;
use crate::platform::{host, process};
use rustix::fs::{FlockOperation, flock};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const FETCH_LIMIT: Duration = Duration::from_secs(120);
const PINNED_LOCK_WAIT: Duration = Duration::from_secs(10 * 60);
const ROLLBACK_LOCK_WAIT: Duration = Duration::from_secs(2 * 60);
const WRITE_LOCK_WAIT: Duration = Duration::from_secs(2 * 60);

const PASSES: usize = 5;
const ABANDONED_COPY: Duration = Duration::from_secs(60 * 60);
const UPDATE_LOCK: &str = "runtime-update.lock";
const PROMOTE_LOCK: &str = "runtime-promote.lock";
const PROMOTE_QUEUE: &str = "runtime-promote-queue.lock";
const CHECKOUT: &str = "code/north/main";
const BUILD_FOR_PEER_LOCK: &str = "runtime-build.lock";
const BUILD_FOR_PEER_WAIT: Duration = Duration::from_secs(30 * 60);

pub(super) mod hold;
mod ahead;
pub(super) use ahead::run as build_ahead;
mod build;
mod preswap;
mod shipped;
mod slot;
mod sync;

use build::{build, export_tree};
use hold::Verdict;
use shipped::{Shipped, shipped};
use sync::{relink, republish_if_unpublished, swap, sync};
pub(super) use sync::{alert_proxy, page_proxy};

enum Pass {
    Settled,
    Promoted,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Flight {
    Flew,
    Joined,
}

pub(super) fn update(layout: &Layout, after_flip: &mut dyn FnMut()) -> Result<Flight, Failure> {
    slot::page_overdue(layout);
    let flown = flight(layout, after_flip);
    host_facts::record_if_stale(layout);
    refresh_shared_config();
    flown
}

/// A caller that waits for the running promotion still dates its landing, so landing to live counts from the landing, not from when the lock came free.
fn note_landing(layout: &Layout) {
    let checkout = checkout();
    if let Ok(main) = main_commit(&checkout) {
        pin::observe(layout, &checkout, &main);
    }
}

/// One promotion runs per host under runtime-promote.lock, which its fetches and builds hold while runtime-update.lock covers only each swap: a caller that finds one running dates its landing and waits for it, then follows origin/main at its newest head, which costs one fetch when the holder already promoted it; a caller still waiting after `PINNED_LOCK_WAIT` leaves its landing to the next tick. `after_flip` runs after each swap, so the lander link follows a runtime as soon as it is live rather than after the holder's last pass.
fn flight(layout: &Layout, after_flip: &mut dyn FnMut()) -> Result<Flight, Failure> {
    if let Some(_promoting) = named_lock(layout, PROMOTE_LOCK, FlockOperation::NonBlockingLockExclusive)? {
        return follow_main(layout, after_flip).map(|()| Flight::Flew);
    }
    if land_offload::settles_fleet_here() {
        note_landing(layout);
    }
    let Some(queued) = named_lock(layout, PROMOTE_QUEUE, FlockOperation::NonBlockingLockExclusive)? else {
        println!("north runtime: another promotion already waits behind the running one; the next tick takes origin/main's newest head");
        return Ok(Flight::Joined);
    };
    let mut follow = north_runtime_follow::Follow::of(layout.current(), std::env::current_exe().ok());
    let waited = named_lock_waiting_unless(layout, PROMOTE_LOCK, remote::limit(PINNED_LOCK_WAIT), "north runtime promote", &mut || {
        slot::page_overdue(layout);
        follow.replaced()
    });
    drop(queued);
    match waited {
        Ok(_promoting) => follow_main(layout, after_flip).map(|()| Flight::Flew),
        Err(held) => {
            println!("north runtime: {}; the next tick takes origin/main's newest head", held.message);
            Ok(Flight::Joined)
        }
    }
}

fn follow_main(layout: &Layout, after_flip: &mut dyn FnMut()) -> Result<(), Failure> {
    for _ in 0..PASSES {
        match pass(layout) {
            Ok(Pass::Promoted) => after_flip(),
            finished => return finished.map(|_| ()),
        }
    }
    Err(failed(DEFERRED, "origin/main kept moving past each build; retrying on the next tick"))
}

pub(super) fn update_to(layout: &Layout, target: &str) -> Result<(), Failure> {
    if !land_offload::accepts_peer_runtimes_here() {
        return Err(failed(1, format!("this host settles the fleet and promotes only a runtime it built or had a build host build, not {} shipped from another host", layout::short(target))));
    }
    let _promoting = promotion_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime update --sha")?;
    let checkout = checkout();
    let previous = fs::read_link(layout.current()).ok();
    match shipped(layout, &checkout, target, previous.as_deref()) {
        Shipped::Current => println!("north runtime current: {}", layout::short(target)),
        Shipped::Older(newer) => println!("north runtime: not moving {} back from {} to {}", host::name(), layout::short(&newer), layout::short(target)),
        Shipped::Missing => return Err(failed(1, format!("{} was not shipped to this host; runtime unchanged", layout::short(target)))),
        Shipped::Promote => return advance(layout, &checkout, target, previous.as_deref()).map(|_| ()),
    }
    Ok(())
}

/// One build for another host runs here at a time, since each borrows the one runtime-build lane and kept target; a commit neither on origin/main nor sent here ahead of its landing as a lander's candidate is refused, so a build host builds only what landed or what a lander is checking.
pub(super) fn build_for_peer(layout: &Layout, target: &str) -> Result<(), Failure> {
    let checkout = checkout();
    let candidate = ahead::sent(&checkout, target);
    let built = build_landed_or_candidate(layout, &checkout, target, candidate);
    if candidate {
        ahead::forget(&checkout, target);
    }
    built
}

fn build_landed_or_candidate(layout: &Layout, checkout: &Path, target: &str, candidate: bool) -> Result<(), Failure> {
    fetch(checkout)?;
    if !candidate && !ancestor(checkout, target, "origin/main") {
        return Err(failed(1, format!("{} is neither on origin/main nor a candidate a lander sent ahead; a build host builds no other commit", layout::short(target))));
    }
    let _building = named_lock_waiting(layout, BUILD_FOR_PEER_LOCK, remote::limit(BUILD_FOR_PEER_WAIT), "north runtime build")?;
    let _writing = writing(layout, target)?;
    build::build_for_peer(layout, checkout, target, &layout.runtime(target))
}

pub(super) fn roll_back(layout: &Layout, failed_sha: &str) -> Result<String, Failure> {
    let _flipping = named_lock_waiting(layout, UPDATE_LOCK, remote::limit(ROLLBACK_LOCK_WAIT), "north runtime rollback").map_err(|held| failed(DEFERRED, format!("{}; the promotion holding it decides what stays current", held.message)))?;
    let current = fs::read_link(layout.current()).ok();
    if current.as_deref() != Some(layout::link(failed_sha).as_path()) {
        return Err(failed(DEFERRED, format!("current is not {}; nothing to roll back", layout::short(failed_sha))));
    }
    if !lifecycle::confirm_current(layout, failed_sha)? {
        return Err(failed(1, format!("{} never finished its promotion; nothing to roll back", layout::short(failed_sha))));
    }
    let Some(previous) = lifecycle::previous(layout, failed_sha, |sha| layout::has_programs(&layout.runtime(sha)))? else {
        return Err(failed(DEFERRED, format!("no previous runtime to roll {} back to; nothing to roll back", layout::short(failed_sha))));
    };
    swap(layout, &layout::link(&previous))?;
    live::note_swap(layout, &layout::link(&previous));
    if fs::read_link(layout.lander()).is_ok_and(|lander| lander == layout::link(failed_sha)) {
        relink(&layout.lander(), &layout::link(&previous))?;
        println!("north runtime: lander link rolled back with current to {previous}");
    }
    lifecycle::move_to::<Promoting>(layout, failed_sha, Promotion::RolledBack, "rolled back on this host after promotion")?;
    lifecycle::move_to::<Promoting>(layout, &previous, Promotion::Live, &format!("rolled back to after {} failed", layout::short(failed_sha)))?;
    println!("north runtime rolled back to {previous}");
    Ok(previous)
}

/// An ordered promotion's go-live on this host: the runtime was built and passed its canary before this, so only the swap is left, and a pass that left `current` elsewhere (held back, rolled back, or moved) is a failure, not a quiet success.
pub(super) fn go_live_ordered(layout: &Layout, target: &str) -> Result<(), Failure> {
    let checkout = checkout();
    let previous = fs::read_link(layout.current()).ok();
    if previous.as_deref() != Some(layout::link(target).as_path()) {
        advance(layout, &checkout, target, previous.as_deref())?;
        if layout.live_sha().as_deref() != Some(target) {
            return Err(failed(1, format!("{} did not go live here: current is {}", layout::short(target), layout.live_sha().as_deref().map_or("missing", layout::short))));
        }
    }
    pin::promoted(layout, target);
    Ok(())
}

pub(super) fn build_ordered(layout: &Layout, target: &str) -> Result<(), Failure> {
    built(layout, &checkout(), target).map(|_| ())
}

pub(super) fn order_waiting(layout: &Layout) -> Result<File, Failure> {
    promotion_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime order")
}

pub(super) fn promotion_waiting(layout: &Layout, limit: Duration, waiter: &str) -> Result<File, Failure> {
    named_lock_waiting(layout, PROMOTE_LOCK, limit, waiter)
}

/// Only the swap holds runtime-update.lock, so a rollback waits for a swap and never for a build; a current a rollback moved while the runtime was built defers it to the next pass rather than swap from a runtime no longer live.
fn flip(layout: &Layout, target: &str, before: Option<&Path>) -> Result<File, Failure> {
    let flipping = named_lock_waiting(layout, UPDATE_LOCK, remote::limit(PINNED_LOCK_WAIT), "north runtime promote")?;
    if fs::read_link(layout.current()).ok().as_deref() != before {
        return Err(failed(DEFERRED, format!("current moved while {} was built; the next pass promotes it from the runtime now live", layout::short(target))));
    }
    Ok(flipping)
}

pub(super) fn named_lock_waiting(layout: &Layout, name: &str, limit: Duration, waiter: &str) -> Result<File, Failure> {
    named_lock_waiting_unless(layout, name, limit, waiter, &mut || None)
}

/// A waiter whose runtime is no longer the one `current` names leaves at once: the promotion holding the lock and the runtime now current carry what it would have promoted.
fn named_lock_waiting_unless(layout: &Layout, name: &str, limit: Duration, waiter: &str, replaced: &mut dyn FnMut() -> Option<String>) -> Result<File, Failure> {
    let started = Instant::now();
    let mut announced = false;
    loop {
        if let Some(held) = named_lock(layout, name, FlockOperation::NonBlockingLockExclusive)? {
            return Ok(held);
        }
        if let Some(now) = replaced() {
            return Err(failed(DEFERRED, format!("{waiter} left its wait for {name}: this runtime is no longer current, {now} is")));
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
    if land_offload::runs_pinned_runtime_here() {
        println!("north runtime: {} runs the pinned runtime the settling host ships it after the runtime walked the spine there; it does not follow origin/main", host::name());
        if let Some(live) = fs::read_link(layout.current()).ok().as_deref().and_then(layout::linked_sha) {
            republish_if_unpublished(layout, live);
        }
        return Ok(Pass::Settled);
    }
    let checkout = checkout();
    let main = main_commit(&checkout)?;
    let settles = land_offload::settles_fleet_here();
    let target = if settles { slot::target(layout, &checkout, &main) } else { main };
    let advanced = advance_to(layout, &checkout, &target);
    if settles && matches!(advanced, Ok(Pass::Promoted)) {
        pin::promoted(layout, &target);
    }
    advanced
}

fn advance_to(layout: &Layout, checkout: &Path, target: &str) -> Result<Pass, Failure> {
    let previous = fs::read_link(layout.current()).ok();
    let linked = previous.as_deref() == Some(layout::link(target).as_path());
    if linked && lifecycle::confirm_current(layout, target)? {
        println!("north runtime current: {}", layout::short(target));
        republish_if_unpublished(layout, target);
        return Ok(Pass::Settled);
    }
    let fallback = match linked {
        true => lifecycle::previous(layout, target, |sha| layout::has_programs(&layout.runtime(sha)))?.map(|sha| layout::link(&sha)),
        false => previous,
    };
    match advance(layout, checkout, target, fallback.as_deref()) {
        Err(deferred) if deferred.code == DEFERRED => catch_up(layout, checkout, target, fallback.as_deref())?.ok_or(deferred),
        advanced => advanced,
    }
}

fn advance(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<Pass, Failure> {
    let short = layout::short(target);
    let was = previous.and_then(layout::linked_sha);
    if let Some(was) = was {
        lifecycle::confirm_current(layout, was)?;
        let host = host::name();
        if land_offload::settles_fleet_here() && layout.live_sha().as_deref() == Some(was) && judge_outgoing(layout, &host, was, &canary::Live { layout, host: &Shell::Here(host.clone()) }) {
            return Ok(Pass::Settled);
        }
    }
    let verdict = hold::verdict(layout, target, usable(&layout.runtime(target)))?;
    if let Some(notice) = verdict.notice(short, was.unwrap_or("none")) {
        println!("{notice}");
    }
    if let Verdict::Held(_) = verdict {
        note_promotion(layout, target, "held");
        return Ok(Pass::Settled);
    }
    let promoted = promote(layout, checkout, target, previous);
    if let Err((stopped, failure)) = &promoted {
        note_promotion(layout, target, "failed");
        let stopped = if failure.code == DEFERRED { Promotion::Unfinished } else { *stopped };
        lifecycle::move_to::<Promoting>(layout, target, stopped, &failure.message)?;
    }
    let (synced, _flipping) = promoted.map_err(|(_, failure)| failure)?;
    lifecycle::went_live(layout, target, was)?;
    note_promotion(layout, target, "live");
    live::note_swap(layout, &layout::link(target));
    println!("{synced}north runtime promoted: {short} (was {})", was.unwrap_or("none"));
    host_facts::record_after_promotion(layout, target);
    collect(layout, target, previous).map_err(|error| failed(1, format!("collecting old runtimes: {error}")))?;
    Ok(Pass::Promoted)
}

/// Judges the runtime a promotion on the walking host is about to replace when no canary has, as when its own walk died or ran no probes: it is the rollback target should the next one fail. A rollback leaves another runtime live, so the promotion waits for the next pass.
pub(super) fn judge_outgoing(layout: &Layout, host: &str, was: &str, machine: &impl canary::Machine) -> bool {
    canary::check(layout, host, was, machine) == canary::Verdict::RolledBack
}

fn catch_up(layout: &Layout, checkout: &Path, main: &str, previous: Option<&Path>) -> Result<Option<Pass>, Failure> {
    let current = previous.and_then(layout::linked_sha);
    let ahead = |sha: &str| sha != main && ancestor(checkout, sha, main) && current.is_none_or(|current| sha != current && !ancestor(checkout, sha, current));
    let promoted = peers::promoted().into_iter().filter(|peer| ahead(&peer.sha)).collect();
    let Some(newest) = peers::newest(promoted, |later, earlier| ancestor(checkout, earlier, later)) else {
        return Ok(None);
    };
    let runtime = layout.runtime(&newest.sha);
    if !usable(&runtime) {
        let _writing = writing(layout, &newest.sha)?;
        if !usable(&runtime) && !pull::pull_from(&newest.host, &newest.sha, &runtime) {
            return Ok(None);
        }
    }
    println!("north runtime: {} is not built anywhere yet; catching up to {}, promoted on {}", layout::short(main), layout::short(&newest.sha), newest.host);
    advance(layout, checkout, &newest.sha, previous)?;
    Ok(Some(Pass::Settled))
}

fn ancestor(checkout: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(checkout), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
}

fn promote(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<(String, File), (Promotion, Failure)> {
    let unfinished = |failure| (Promotion::Unfinished, failure);
    let before = fs::read_link(layout.current()).ok();
    let building = !usable(&layout.runtime(target));
    if building {
        lifecycle::move_to::<Promoting>(layout, target, Promotion::Building, "pulling a peer's build or building here").map_err(unfinished)?;
        note_promotion(layout, target, "building");
    }
    let runtime = built(layout, checkout, target).map_err(|failure| (Promotion::BuildFailed, failure))?;
    let flipping = flip(layout, target, before.as_deref()).map_err(unfinished)?;
    let cause = if building { "built; swapping current to it and checking it" } else { "built elsewhere and shipped; swapping current to it and checking it" };
    lifecycle::move_to::<Promoting>(layout, target, Promotion::Installing, cause).map_err(unfinished)?;
    note_promotion(layout, target, "installing");
    let synced = install(layout, target, &runtime, previous).map_err(|failure| (if failure.denied { Promotion::Unfinished } else { Promotion::InstallFailed }, failure))?;
    Ok((synced, flipping))
}

fn note_promotion(layout: &Layout, target: &str, state: &str) {
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let record = serde_json::json!({"sha": target, "state": state, "at": at});
    let _ = fs::write(layout.state.join("runtime-promote.json"), format!("{record}\n"));
}

fn install(layout: &Layout, target: &str, runtime: &Path, previous: Option<&Path>) -> Result<String, Failure> {
    layout::built_from_own_commit(runtime).map_err(|why| failed(1, why))?;
    super::entry_links::link(runtime)?;
    store::root(runtime)?;
    if let Err(failure) = store::shims(runtime) {
        eprintln!("north runtime update: the operator terminal keeps its previous shims: {}", failure.message);
    }
    super::deploy::stage_for_lander(layout, target, runtime);
    let synced = sync(layout, previous, target, &mut |text: &str| alert_proxy(&format!("north runtime update: {text}")))?;
    Ok(synced)
}

pub(super) fn named_lock(layout: &Layout, name: &str, operation: FlockOperation) -> Result<Option<File>, Failure> {
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
    if usable(&runtime) && runtime.join(TREE).is_dir() {
        return Ok(runtime);
    }
    let _writing = writing(layout, target)?;
    if !usable(&runtime) && !adopted(layout, checkout, target, &runtime) && !reused(layout, checkout, target, &runtime) {
        build(layout, checkout, target, &runtime)?;
    }
    if !runtime.join(TREE).is_dir() {
        export_tree(checkout, target, &runtime)?;
    }
    Ok(runtime)
}

/// Whoever writes `runtimes/SHA` holds its lock, and the install of a runtime shipped from another host takes the same one (deploy::install_script), so a shipped copy never moves a directory a reuse, build, pull or tree export is part-way through filling.
fn writing(layout: &Layout, target: &str) -> Result<File, Failure> {
    named_lock_waiting(layout, &layout::writing_lock(target), remote::limit(WRITE_LOCK_WAIT), "north runtime promote")
}

fn usable(runtime: &Path) -> bool {
    layout::usable(runtime, land_offload::settles_fleet_here())
}

/// Installs the release north built ahead for the landed commit's tree, when one is ready.
fn adopted(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> bool {
    ahead::ready_for(layout, checkout, target).is_some_and(|tree| ahead::adopt(layout, runtime, &tree, target))
}

fn reused(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> bool {
    reuse::reuse(layout, checkout, target, runtime, land_offload::settles_fleet_here())
}

pub(super) fn checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| layout::home().join(CHECKOUT))
}

pub(super) fn git<S: AsRef<OsStr>>(checkout: &Path, args: &[S]) -> Result<String, Failure> {
    let output = process::run("git", Some(checkout), args)?;
    if !output.status.success() {
        let shown = args.iter().map(|arg| arg.as_ref().to_string_lossy()).collect::<Vec<_>>();
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(failed(1, format!("git {} failed: {}", shown.join(" "), stderr.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
    let rollback = fs::read_to_string(layout.lander_rollback()).ok();
    keep.extend(rollback.as_deref().and_then(|target| layout::linked_sha(Path::new(target.trim()))));
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
        let _ = fs::remove_file(layout.state.join(layout::writing_lock(name)));
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

pub(super) fn fetch(checkout: &Path) -> Result<(), Failure> {
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

fn refresh_shared_config() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let now = crate::platform::records_mirror::now() as u64;
    let Some(refreshed) = crate::platform::shared_config::refresh_if_due(&home, now) else { return };
    if let Err(error) = refreshed {
        eprintln!("north runtime update: {error}");
    }
    if let Err(stale) = crate::platform::shared_config::fresh(&home, now) {
        eprintln!("north runtime update: {stale}");
    }
}
