use super::super::cargo_build::{self, BuildLog};
use super::super::failure::{DEFERRED, Failure, failed};
use super::super::layout::{self, BUILT_HERE, Layout, PROGRAMS, TREE};
use super::super::pull;
use super::super::remote;
use super::git;
use crate::capacity::game_reserve::Confinement;
use crate::land_offload;
use crate::pacing::placement::gate;
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

pub(super) fn build(layout: &Layout, checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
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
    let kept = layout::home().join(".cache/cargo-target/north");
    let built = lane.path.join("target");
    let _kept = KeptTarget::borrow(&kept, &built);
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
    let release = built.join("release");
    if !layout::has_programs(&release) {
        return Err(failed(DEFERRED, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        layout::install(&release.join(program), &runtime.join(program))?;
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

