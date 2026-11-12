use super::super::cargo_build::{self, BuildLog};
use super::super::failure::{DEFERRED, Failure, failed};
use super::super::layout::{self, BUILT_HERE, Layout, PROGRAMS, TREE};
use super::super::build_host;
use super::super::pull;
use super::git;
use crate::capacity::game_reserve::Confinement;
use crate::land_offload;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl Drop for Lane<'_> {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Depbox builds into the lane's own `target`, so the lane borrows the kept incremental target for the build and hands it back after, instead of cargo building into a path nothing reads.
struct KeptTarget<'a> {
    kept: &'a Path,
    lent: &'a Path,
}

impl<'a> KeptTarget<'a> {
    fn borrow(kept: &'a Path, lent: &'a Path) -> Self {
        if kept.is_dir() && !lent.exists() {
            let _ = fs::rename(kept, lent);
        }
        Self { kept, lent }
    }
}

impl Drop for KeptTarget<'_> {
    fn drop(&mut self) {
        if self.lent.is_dir() {
            if let Some(parent) = self.kept.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(self.lent, self.kept);
        }
    }
}

enum Placement {
    Here { reserved: bool },
    Confined(Confinement),
    Held,
}

fn placement(reserved: bool, settles: bool, confinement: impl FnOnce() -> Option<Confinement>) -> Placement {
    match (reserved, settles) {
        (true, true) => confinement().map_or(Placement::Held, Placement::Confined),
        (reserved, _) => Placement::Here { reserved },
    }
}

/// A runtime nobody has built yet: pulled from a peer where this host takes them, else built on the build host with the most free cores, else here, which the log line says.
pub(super) fn build(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    if pull::pull(target, runtime) {
        return Ok(());
    }
    let short = layout::short(target);
    let refused = match build_host::build(target, runtime, land_offload::build_host_rooms(), build_host::build_on) {
        Ok(built) => {
            build_host::note(layout, target, built);
            return Ok(());
        }
        Err(why) => why,
    };
    let mut confined = None;
    match placement(land_offload::in_game_reserve(), land_offload::settles_fleet_here(), crate::capacity::game_reserve::confinement) {
        Placement::Confined(confinement) => {
            println!("north runtime: {refused}; this host is in game-reserve, so it builds {short} on the agent CPUs {} at idle priority", confinement.cpus);
            confined = Some(confinement);
        }
        Placement::Held => return Err(failed(DEFERRED, format!("{refused}, and this host builds its own runtime and is in game-reserve with the agent CPUs not enforced; {short} waits for the reserve to end and the live runtime stays"))),
        Placement::Here { reserved: true } => println!("north runtime: {refused}; this host is in game-reserve and builds {short} here under the runtime class admission"),
        Placement::Here { reserved: false } => println!("north runtime: {refused}; building {short} on this host"),
    }
    if !land_offload::builds_runtime_here() {
        return Err(failed(DEFERRED, format!("{refused}, and this host does not build runtimes (builds_runtime = false); retrying {short} on the next tick")));
    }
    let started = std::time::Instant::now();
    build_here(layout, checkout, target, runtime, confined.as_ref())?;
    let binary = layout::binary_hash(&runtime.join("north"))?;
    build_host::note(layout, target, build_host::Built { host: crate::platform::host::name(), seconds: started.elapsed().as_secs() as i64, binary });
    Ok(())
}

/// On a build host: builds `target`, a commit on origin/main, into this host's runtimes for the settling host to take, under this host's runtime capacity admission, without promoting it here.
pub(super) fn build_for_peer(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    if !land_offload::builds_for_peers() {
        return Err(failed(1, format!("{} is no build host (no offload_builds), so it builds no runtime for another host", crate::platform::host::name())));
    }
    let short = layout::short(target);
    if layout::has_programs(runtime) && layout::built_from_own_commit(runtime).is_ok() {
        println!("north runtime: {short} is already built here");
        return Ok(());
    }
    build_here(layout, checkout, target, runtime, None)?;
    println!("north runtime: built {short} for another host to take");
    Ok(())
}

fn build_here(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, confined: Option<&Confinement>) -> Result<(), Failure> {
    let lane = Lane::add(checkout, target)?;
    let kept = layout::home().join(".cache/cargo-target/north");
    let built = lane.path.join("target");
    let _kept = KeptTarget::borrow(&kept, &built);
    let system = north_build_system::here();
    let request = cargo_build::release_build(&lane.path, &built, confined);
    let invocation = system.build(&request);
    let mut build = Command::new(north_base::platform::north_binary::north_binary());
    build
        .args(["capacity", "host", "run", "--class", "runtime", "--owner", "north-runtime", "--timeout-seconds", "1500", "--"])
        .args(cargo_build::idle(confined))
        .args(&invocation.words);
    invocation.environ(&mut build);
    let log = BuildLog::open(layout, target)?;
    let mut attempt = || -> Result<_, Failure> {
        let wall = std::time::SystemTime::now();
        log.attach(&mut build)?;
        let status = build.status()?;
        Ok((status, system.compiled_since(&request, wall)))
    };
    let (mut status, mut ran) = attempt()?;
    if let Some(archives) = log.stale_archives().filter(|_| !status.success() && ran) {
        println!(
            "north runtime: build of {} failed to link against stale release incremental output (build log: {}); clearing the incremental output of the crates behind {} stale rlibs and building once more",
            layout::short(target),
            log.path().display(),
            archives.len()
        );
        cargo_build::recompile_from_scratch(&built, &archives)?;
        (status, ran) = attempt()?;
    }
    if !status.success() || !ran {
        return Err(cargo_build::build_failure(target, status, ran, &log));
    }
    log.discard();
    if !PROGRAMS.iter().all(|program| system.built(&request, program).is_file()) {
        return Err(failed(DEFERRED, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        layout::install(&system.built(&request, program), &runtime.join(program))?;
    }
    fs::write(runtime.join(BUILT_HERE), layout::short(target))?;
    layout::stamp(runtime, target)?;
    Ok(())
}

pub(super) fn export_tree(checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
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

