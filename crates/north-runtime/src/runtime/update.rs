use super::bounded;
use super::host_facts;
use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, BUILT_HERE, Layout, PROGRAMS, TREE};
use super::peers;
use super::pull;
use super::queue::{self, Entry};
use super::remote;
use super::reuse;
use super::store;
use super::supersede::{self, SUPERSEDED};
use crate::capacity::game_reserve::Confinement;
use crate::land_offload;
use crate::pacing::placement::gate;
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STEP_LIMIT: Duration = Duration::from_secs(300);
const FETCH_LIMIT: Duration = Duration::from_secs(120);
const PINNED_LOCK_WAIT: Duration = Duration::from_secs(10 * 60);
const ROLLBACK_LOCK_WAIT: Duration = Duration::from_secs(2 * 60);

const PASSES: usize = 5;
const UPDATE_LOCK: &str = "runtime-update.lock";
pub(super) const DEPLOY_LOCK: &str = "runtime-deploy.lock";

pub(super) mod hold;

use hold::{Cause, Verdict};

enum Pass {
    Settled,
    Promoted,
}

pub(super) fn update(layout: &Layout) -> Result<(), Failure> {
    let Some(_lock) = lock(layout, FlockOperation::NonBlockingLockExclusive)? else {
        println!("north runtime update already running");
        return Ok(());
    };
    for _ in 0..PASSES {
        match pass(layout) {
            Err(failure) if failure.code == SUPERSEDED => println!("north runtime: {}; restarting on the new origin/main", failure.message),
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
    let previous = fs::read_to_string(layout.previous()).map(|sha| sha.trim().to_owned()).ok().filter(|sha| layout::is_sha(sha) && sha != failed_sha);
    let Some(previous) = previous.filter(|sha| layout::has_programs(&layout.runtime(sha))) else {
        return Err(failed(1, format!("no previous runtime to roll {} back to", layout::short(failed_sha))));
    };
    swap(layout, &layout::link(&previous))?;
    hold::record(layout, failed_sha, Cause::Rollback, "rolled back on this host after promotion")?;
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
    if previous.as_deref() == Some(layout::link(&target).as_path()) {
        println!("north runtime current: {}", layout::short(&target));
        return Ok(Pass::Settled);
    }
    let wanted = next_build(layout, &checkout, &target, previous.as_deref());
    match advance(layout, &checkout, &wanted, previous.as_deref()) {
        Err(deferred) if deferred.code == DEFERRED => catch_up(layout, &checkout, &target, previous.as_deref())?.ok_or(deferred),
        advanced => advanced,
    }
}

fn advance(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<Pass, Failure> {
    let short = layout::short(target);
    let was = previous.and_then(layout::linked_sha).unwrap_or("none");
    let verdict = hold::verdict(layout, target, usable(&layout.runtime(target)));
    if let Some(notice) = verdict.notice(short, was) {
        println!("{notice}");
    }
    if let Verdict::Held(_) = verdict {
        return Ok(Pass::Settled);
    }
    let promoted = promote(layout, checkout, target, previous);
    if let Err((cause, failure)) = &promoted
        && failure.code != DEFERRED
        && failure.code != SUPERSEDED
    {
        hold::record(layout, target, *cause, &failure.message)?;
    }
    let synced = promoted.map_err(|(_, failure)| failure)?;
    if let Some(was) = previous.and_then(layout::linked_sha) {
        fs::write(layout.previous(), was)?;
    }
    println!("{synced}north runtime promoted: {short} (was {was})");
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

fn promote(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<String, (Cause, Failure)> {
    let runtime = built(layout, checkout, target).map_err(|failure| (Cause::Build, failure))?;
    install(layout, checkout, target, &runtime, previous).map_err(|failure| (Cause::Promote, failure))
}

fn install(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, previous: Option<&Path>) -> Result<String, Failure> {
    if !runtime.join(TREE).is_dir() {
        export_tree(checkout, target, runtime)?;
    }
    store::root(runtime)?;
    sync(layout, previous, target)
}

fn skipped(layout: &Layout) -> Vec<String> {
    let journal = fs::read_to_string(layout.abandoned()).unwrap_or_default();
    journal.lines().rev().filter_map(|line| line.split_whitespace().nth(1)).filter(|sha| layout::is_sha(sha)).map(str::to_owned).collect()
}

fn next_build(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> String {
    let current = previous.and_then(layout::linked_sha);
    let failed = hold::failed_shas(layout);
    let mut entries = vec![Entry { sha: target.to_owned(), failed: failed.iter().any(|sha| sha == target) }];
    for sha in skipped(layout) {
        let ahead = sha != target && current.is_none_or(|current| sha != current && !ancestor(checkout, &sha, current)) && ancestor(checkout, &sha, target);
        if ahead && !entries.iter().any(|entry| entry.sha == sha) {
            entries.push(Entry { failed: failed.contains(&sha), sha });
        }
    }
    queue::next(&entries, |older, newer| ancestor(checkout, older, newer)).unwrap_or(target).to_owned()
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
    Elsewhere(gate::Machine),
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
        Ok(machine) => Placement::Elsewhere(machine),
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
    let lane = Lane::add(checkout, target)?;
    let built = layout::home().join(".cache/cargo-target/north");
    let mut build = Command::new("bun");
    build
        .arg(capacity_helper()?)
        .args(["run", "--class", "runtime", "--owner", "north-runtime", "--timeout-seconds", "1500", "--"])
        .args(cargo_words(confined.as_ref()))
        .current_dir(&lane.path)
        .env("CARGO_TARGET_DIR", &built)
        .env("CARGO_PROFILE_RELEASE_INCREMENTAL", "true");
    let previous = fs::read_to_string(layout.build_seconds()).ok().and_then(|seconds| seconds.trim().parse().ok()).map(Duration::from_secs);
    let started = Instant::now();
    let wall = std::time::SystemTime::now();
    let status = supersede::run(&mut build, target, previous, || main_commit(checkout).ok()).inspect_err(|failure| {
        if failure.code == SUPERSEDED {
            note_abandoned(layout, target);
        }
    })?;
    if !status.success() {
        return Err(build_failure(target, status, cargo_ran(&built, wall)));
    }
    let release = built.join("release");
    if !layout::has_programs(&release) {
        return Err(failed(DEFERRED, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        layout::install(&release.join(program), &runtime.join(program))?;
    }
    fs::write(runtime.join(BUILT_HERE), layout::short(target))?;
    fs::write(layout.build_seconds(), started.elapsed().as_secs().to_string())?;
    Ok(())
}

fn cargo_words(confined: Option<&Confinement>) -> Vec<String> {
    let cargo = ["nix", "develop", "-c", "cargo", "build", "--release", "--locked", "--timings"].map(str::to_owned);
    match confined {
        Some(confinement) => ["nice", "-n", "19", "ionice", "-c", "3"].map(str::to_owned).into_iter().chain(cargo).chain(["-j".to_owned(), confinement.jobs.to_string()]).collect(),
        None => cargo.to_vec(),
    }
}

fn cargo_ran(built: &Path, since: std::time::SystemTime) -> bool {
    fs::metadata(built.join("cargo-timings/cargo-timing.html")).and_then(|metadata| metadata.modified()).is_ok_and(|written| written >= since)
}

fn build_failure(target: &str, status: std::process::ExitStatus, cargo_ran: bool) -> Failure {
    match cargo_ran {
        true => failed(code(status), format!("build of {target} failed: {status}")),
        false => failed(DEFERRED, format!("capacity admission or its scope stopped the build of {} before cargo ran ({status}); retrying on the next tick", layout::short(target))),
    }
}

fn capacity_helper() -> Result<PathBuf, Failure> {
    let output = process::run("agents", None, &["path", "machine-capacity"])?;
    let skill = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    match skill.parent() {
        Some(skill_dir) if output.status.success() => Ok(skill_dir.join("scripts/machine-capacity.mjs")),
        _ => Err(failed(1, "agents path machine-capacity named no skill")),
    }
}

fn note_abandoned(layout: &Layout, target: &str) {
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = format!("{at} {target}\n");
    let _ = fs::OpenOptions::new().create(true).append(true).open(layout.abandoned()).and_then(|mut journal| std::io::Write::write_all(&mut journal, line.as_bytes()));
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

fn swap(layout: &Layout, link: &Path) -> std::io::Result<()> {
    let staged = layout.state.join("current.tmp");
    let _ = fs::remove_file(&staged);
    symlink(link, &staged)?;
    fs::rename(&staged, layout.current())
}

fn sync(layout: &Layout, previous: Option<&Path>, target: &str) -> Result<String, Failure> {
    let log = tempfile::Builder::new()
        .prefix(&format!("runtime-sync-{}.", &target[..8]))
        .suffix(".log")
        .tempfile_in(&layout.state)?;
    swap(layout, &layout::link(target))?;
    let steps = [("health check", health_check(layout)), ("agents sync", agents_sync())];
    let mut failure = None;
    for (name, mut command) in steps {
        let ran = log.reopen().and_then(|out| {
            let err = log.reopen()?;
            bounded::status(command.stdout(out).stderr(err), STEP_LIMIT)
        });
        match ran {
            Ok(status) if status.success() => continue,
            Ok(status) => failure = Some((name, code(status))),
            Err(error) => {
                let _ = fs::write(log.path(), format!("{name} did not run: {error}\n"));
                failure = Some((name, 127));
            }
        }
        break;
    }
    let Some((step, status)) = failure else {
        let mut synced = fs::read_to_string(log.path()).unwrap_or_default();
        if !synced.is_empty() && !synced.ends_with('\n') {
            synced.push('\n');
        }
        return Ok(synced);
    };
    if let Some(previous) = previous {
        swap(layout, previous)?;
    }
    let (_, kept) = log.keep().map_err(|error| failed(1, error.to_string()))?;
    let was = previous.map_or_else(|| "<none>".to_owned(), |link| link.display().to_string());
    let message = format!(
        "failed step {step} on {target}; rolled back to {was}; exit status {status}; local log: {}",
        kept.display()
    );
    alert(&message);
    Err(failed(1, message))
}

fn health_check(layout: &Layout) -> Command {
    let mut command = Command::new(layout.current().join("north"));
    command.args(["runtime", "health"]);
    command
}

fn agents_sync() -> Command {
    let mut command = Command::new("agents");
    command.arg("sync");
    command
}

fn alert(message: &str) {
    alert_proxy(&format!("north runtime update: {message}"));
}

pub(super) fn alert_proxy(text: &str) {
    let sent = Command::new("north")
        .args(["fleet", "send", "--from", "service:north-runtime-update", "proxy", text])
        .status();
    if !sent.is_ok_and(|status| status.success()) {
        eprintln!("north runtime update: rollback alert delivery failed");
    }
}

fn collect(layout: &Layout, target: &str, previous: Option<&Path>) -> std::io::Result<()> {
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
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), doomed.as_os_str()])?;
        fs::remove_dir_all(&doomed)?;
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
mod tests {
    use super::*;

    #[test]
    fn collecting_old_runtimes_keeps_the_runtime_the_lander_link_pins() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_path_buf() };
        let shas: Vec<String> = (0..7).map(|digit| digit.to_string().repeat(40)).collect();
        for sha in &shas {
            fs::create_dir_all(layout.runtime(sha)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::os::unix::fs::symlink(layout::link(&shas[0]), layout.lander()).unwrap();
        std::os::unix::fs::symlink(layout::link(&shas[6]), layout.current()).unwrap();
        collect(&layout, &shas[6], None).unwrap();
        assert!(layout.runtime(&shas[0]).is_dir());
        assert!(!layout.runtime(&shas[1]).exists());
    }

    fn machine(name: &str) -> gate::Machine {
        gate::Machine { name: name.to_owned(), ssh: name.to_owned(), serves: Vec::new(), pushes: None, reaches: None, cap: 0, live: None, accounts: Vec::new(), saturation: None }
    }

    #[test]
    fn a_game_reserve_host_with_no_other_host_with_room_builds_here_under_local_admission() {
        let refused = "no other host has room: gex45: CPU pressure 63% at the 40% admission mark".to_owned();
        match placement(true, false, || panic!("not asked"), || Err(refused.clone())) {
            Placement::Here { refused: Some(why) } => assert_eq!(why, refused),
            _ => panic!("a reserved host with no room elsewhere must build locally, not defer"),
        }
    }

    fn agent_cpus() -> Confinement {
        Confinement { cpus: "4-7".to_owned(), jobs: 4 }
    }

    #[test]
    fn a_game_reserve_host_that_builds_only_its_own_runtime_builds_confined_without_asking_another_host() {
        match placement(true, true, || Some(agent_cpus()), || panic!("no other host is asked")) {
            Placement::Confined(confinement) => assert_eq!((confinement.cpus.as_str(), confinement.jobs), ("4-7", 4)),
            _ => panic!("a reserved settler must build here on the agent CPUs, not defer and not go elsewhere"),
        }
        assert!(matches!(placement(false, true, || panic!("not asked"), || panic!("not asked")), Placement::Here { refused: None }));
    }

    #[test]
    fn a_game_reserve_settler_whose_agent_cpus_are_not_enforced_still_holds_the_build() {
        assert!(matches!(placement(true, true, || None, || panic!("no other host is asked")), Placement::Held));
    }

    #[test]
    fn a_confined_build_runs_cargo_at_idle_priority_with_jobs_capped_at_the_agent_cpus() {
        let words = cargo_words(Some(&agent_cpus()));
        assert_eq!(words[..6], ["nice", "-n", "19", "ionice", "-c", "3"]);
        assert_eq!(words[words.len() - 2..], ["-j", "4"]);
        assert!(!cargo_words(None).iter().any(|word| word == "nice" || word == "-j"));
    }

    #[test]
    fn a_game_reserve_host_builds_on_another_host_that_has_room() {
        match placement(true, false, || panic!("not asked"), || Ok(machine("gex45"))) {
            Placement::Elsewhere(chosen) => assert_eq!(chosen.name, "gex45"),
            _ => panic!("a reserved host with room elsewhere must build there"),
        }
    }
}
