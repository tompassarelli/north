use super::bounded;
use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, Layout, PROGRAMS, TREE};
use super::peers;
use super::pull;
use super::remote;
use super::store;
use super::supersede::{self, SUPERSEDED};
use crate::ops::land_offload;
use crate::ops::pacing::placement::gate;
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STEP_LIMIT: Duration = Duration::from_secs(300);
const FETCH_LIMIT: Duration = Duration::from_secs(120);
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
const PINNED_LOCK_WAIT: Duration = Duration::from_secs(10 * 60);

const PASSES: usize = 5;

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
    let _lock = lock_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime update --sha")?;
    let checkout = checkout();
    fetch(&checkout)?;
    let previous = fs::read_link(layout.current()).ok();
    if previous.as_deref() == Some(layout::link(target).as_path()) {
        println!("north runtime current: {}", layout::short(target));
        return Ok(());
    }
    if !layout::has_programs(&layout.runtime(target)) {
        return Err(failed(1, format!("{} was not shipped to this host; runtime unchanged", layout::short(target))));
    }
    advance(layout, &checkout, target, previous.as_deref()).map(|_| ())
}

pub(super) fn lock_waiting(layout: &Layout, limit: Duration, waiter: &str) -> Result<File, Failure> {
    let started = Instant::now();
    let mut announced = false;
    loop {
        if let Some(held) = lock(layout, FlockOperation::NonBlockingLockExclusive)? {
            return Ok(held);
        }
        if !announced {
            println!("{waiter}: waiting up to {}s for runtime-update.lock, held by another north runtime update", limit.as_secs());
            announced = true;
        }
        if started.elapsed() >= limit {
            return Err(failed(1, format!("runtime-update.lock stayed held by another north runtime update for {}s", limit.as_secs())));
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
    match advance(layout, &checkout, &target, previous.as_deref()) {
        Err(deferred) if deferred.code == DEFERRED => catch_up(layout, &checkout, &target, previous.as_deref())?.ok_or(deferred),
        advanced => advanced,
    }
}

fn advance(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<Pass, Failure> {
    let short = layout::short(target);
    let was = previous.and_then(layout::linked_sha).unwrap_or("none");
    if held_back(layout, target) {
        println!("north runtime: {short} failed within the last hour; staying on {was}");
        return Ok(Pass::Settled);
    }
    let promoted = promote(layout, checkout, target, previous);
    if let Err(failure) = &promoted
        && failure.code != DEFERRED
        && failure.code != SUPERSEDED
    {
        fs::write(layout.failed(), target)?;
    }
    let synced = promoted?;
    println!("{synced}north runtime promoted: {short} (was {was})");
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
    if !layout::has_programs(&runtime) && !pull::pull_from(&newest.host, &newest.sha, &runtime) {
        return Ok(None);
    }
    println!("north runtime: {} is not built anywhere yet; catching up to {}, promoted on {}", layout::short(main), layout::short(&newest.sha), newest.host);
    advance(layout, checkout, &newest.sha, previous)?;
    Ok(Some(Pass::Settled))
}

fn ancestor(checkout: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(checkout), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
}

fn promote(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Result<String, Failure> {
    let runtime = built(layout, checkout, target)?;
    if !runtime.join(TREE).is_dir() {
        export_tree(checkout, target, &runtime)?;
    }
    store::root(&runtime)?;
    sync(layout, previous, target)
}

fn held_back(layout: &Layout, target: &str) -> bool {
    let failed = layout.failed();
    let recent = fs::metadata(&failed)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < RETRY_AFTER));
    recent && fs::read_to_string(&failed).is_ok_and(|sha| sha.trim() == target)
}

pub(super) fn lock(layout: &Layout, operation: FlockOperation) -> Result<Option<File>, Failure> {
    fs::create_dir_all(layout.runtimes())?;
    let lock = File::create(layout.state.join("runtime-update.lock"))?;
    Ok(flock(&lock, operation).is_ok().then_some(lock))
}

pub(super) fn built_main(layout: &Layout, checkout: &Path) -> Result<(String, PathBuf), Failure> {
    let target = main_commit(checkout)?;
    if !layout::has_programs(&layout.runtime(&target)) {
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
    if !layout::has_programs(&runtime) {
        build(layout, checkout, target, &runtime)?;
    }
    Ok(runtime)
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
        let path = layout::home().join(format!("code/north/worktrees/runtime-{}", &target[..8]));
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
}

fn placement(reserved: bool, other_host: impl FnOnce() -> Result<gate::Machine, String>) -> Placement {
    if !reserved {
        return Placement::Here { refused: None };
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
    match placement(land_offload::in_game_reserve(), land_offload::host_with_room) {
        Placement::Elsewhere(machine) => return remote::build(&machine, target, runtime),
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
        .args(["nix", "develop", "-c", "cargo", "build", "--release", "--locked"])
        .current_dir(&lane.path)
        .env("CARGO_TARGET_DIR", &built);
    let previous = fs::read_to_string(layout.build_seconds()).ok().and_then(|seconds| seconds.trim().parse().ok()).map(Duration::from_secs);
    let started = Instant::now();
    let status = supersede::run(&mut build, target, previous, || main_commit(checkout).ok())?;
    if !status.success() {
        return Err(failed(code(status), format!("build of {target} failed: {status}")));
    }
    let release = built.join("release");
    if !layout::has_programs(&release) {
        return Err(failed(DEFERRED, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        install(&release.join(program), &runtime.join(program))?;
    }
    fs::write(layout.build_seconds(), started.elapsed().as_secs().to_string())?;
    Ok(())
}

fn capacity_helper() -> Result<PathBuf, Failure> {
    let output = process::run("agents", None, &["path", "machine-capacity"])?;
    let skill = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    match skill.parent() {
        Some(skill_dir) if output.status.success() => Ok(skill_dir.join("scripts/machine-capacity.mjs")),
        _ => Err(failed(1, "agents path machine-capacity named no skill")),
    }
}

fn install(from: &Path, to: &Path) -> std::io::Result<()> {
    let staged = to.with_extension("tmp");
    fs::copy(from, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    fs::rename(&staged, to)
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
    let sent = Command::new("north")
        .args(["fleet", "send", "proxy", &format!("north runtime update: {message}")])
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

    fn machine(name: &str) -> gate::Machine {
        gate::Machine { name: name.to_owned(), ssh: name.to_owned(), serves: Vec::new(), pushes: None, cap: 0, live: None, accounts: Vec::new(), saturation: None }
    }

    #[test]
    fn a_game_reserve_host_with_no_other_host_with_room_builds_here_under_local_admission() {
        let refused = "no other host has room: gex45: CPU pressure 63% at the 40% admission mark".to_owned();
        match placement(true, || Err(refused.clone())) {
            Placement::Here { refused: Some(why) } => assert_eq!(why, refused),
            _ => panic!("a reserved host with no room elsewhere must build locally, not defer"),
        }
    }

    #[test]
    fn a_game_reserve_host_builds_on_another_host_that_has_room() {
        match placement(true, || Ok(machine("gex45"))) {
            Placement::Elsewhere(chosen) => assert_eq!(chosen.name, "gex45"),
            _ => panic!("a reserved host with room elsewhere must build there"),
        }
    }
}
