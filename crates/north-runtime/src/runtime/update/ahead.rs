//! Release runtimes built ahead of the landing they belong to: the lander asks for the release north of a queued
//! lane's candidate tree, and the promotion installs it by the tree hash the landed commit carries instead of
//! building the same tree again after the push. An artifact is kept under `runtime-artifacts/<tree>`: the runtime
//! programs, the commit it was built from, and when. One build runs at a time, and only when no artifact for that
//! tree is ready yet.
use super::super::cargo_build::{self, BuildLog};
use super::super::failure::{DEFERRED, Failure, failed};
use super::super::layout::{self, BUILT_FROM, BUILT_HERE, Layout, PROGRAMS};
use super::super::pin::{self, Event};
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BUILT_AT: &str = "built-at";
const BUILD_LOCK: &str = "build.lock";
const KEPT: usize = 3;
const BUILD_TIMEOUT_SECONDS: &str = "1500";
const OWNER: &str = "north-runtime-ahead";

/// The release north kept for the tree `rev` in `checkout` carries, when it is ready: the artifact is the whole
/// install, so a promotion can copy it and skip the build that would otherwise delay the landing's live runtime.
pub(super) fn ready_for(layout: &Layout, checkout: &Path, rev: &str) -> Option<String> {
    let tree = tree_of(checkout, rev).ok()?;
    ready(layout, &tree).map(|_| tree)
}

/// The tree hash of `rev`, the identity a candidate carries from the lander's queue to the landed commit.
pub(super) fn tree_of(checkout: &Path, rev: &str) -> Result<String, Failure> {
    let output = process::run("git", Some(checkout), &["rev-parse", &format!("{rev}^{{tree}}")])?;
    if !output.status.success() {
        return Err(failed(1, format!("git rev-parse {rev}^{{tree}}: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    layout::is_sha(&tree).then_some(tree).ok_or_else(|| failed(1, format!("git rev-parse {rev}^{{tree}} printed no tree hash")))
}

/// The artifact for `tree`, when it holds every runtime program.
pub(super) fn ready(layout: &Layout, tree: &str) -> Option<PathBuf> {
    let artifact = layout.artifacts().join(tree);
    PROGRAMS.iter().all(|program| artifact.join(program).is_file()).then_some(artifact)
}

/// Installs the artifact for `tree` as `runtime`, named for the landed commit `target`, when it is ready; whether it did.
pub(super) fn adopt(layout: &Layout, runtime: &Path, tree: &str, target: &str) -> bool {
    let Some(artifact) = ready(layout, tree) else { return false };
    let installed = install(&artifact, runtime, target);
    if installed {
        let _ = fs::remove_dir_all(&artifact);
        pin::note(layout, &Event::ArtifactReady { sha: target.to_owned(), tree: tree.to_owned(), seconds: 0, at: pin::now() });
        println!("north runtime: artifact-ready 0 s: the release north for tree {} was built ahead while its lane queued", layout::short(tree));
    }
    installed
}

fn install(artifact: &Path, runtime: &Path, target: &str) -> bool {
    let Ok(()) = fs::create_dir_all(runtime) else { return false };
    for program in PROGRAMS {
        if layout::install(&artifact.join(program), &runtime.join(program)).is_err() {
            return false;
        }
    }
    if layout::stamp(runtime, target).is_err() {
        return false;
    }
    fs::write(runtime.join(BUILT_HERE), layout::short(target)).is_ok()
}

/// Builds the release north for `tree` from `commit` in `checkout` and keeps it as the promotion's artifact; a tree
/// another build already fills, or one ready already, costs nothing.
pub(super) fn build(layout: &Layout, checkout: &Path, tree: &str, commit: &str) -> Result<(), Failure> {
    if !layout::is_sha(tree) || !layout::is_sha(commit) {
        return Err(failed(64, "build-ahead needs a tree hash and a commit hash"));
    }
    let artifacts = layout.artifacts();
    fs::create_dir_all(&artifacts)?;
    let Some(_lock) = take(&artifacts.join(BUILD_LOCK)) else {
        println!("north runtime build-ahead: could not lock {}; leaving tree {} to the next tick", artifacts.display(), layout::short(tree));
        return Ok(());
    };
    if ready(layout, tree).is_some() {
        println!("north runtime build-ahead: tree {} is ready already", layout::short(tree));
        return Ok(());
    }
    let root = layout::home().join(".cache/north/build-ahead");
    let worktree = root.join("worktree");
    let target = root.join("target");
    let release = build_release(layout, checkout, tree, commit, &worktree, &target)?;
    keep(tree, commit, &release, &artifacts);
    if ready(layout, tree).is_none() {
        return Err(failed(1, format!("could not keep the release build for tree {} under {}", layout::short(tree), artifacts.display())));
    }
    prune(&artifacts);
    println!("north runtime build-ahead: tree {} ready for the promotion of {}", layout::short(tree), layout::short(commit));
    Ok(())
}

fn build_release(layout: &Layout, checkout: &Path, tree: &str, commit: &str, worktree: &Path, target: &Path) -> Result<PathBuf, Failure> {
    let _ = fs::remove_dir_all(worktree);
    let _ = process::run("git", Some(checkout), &["worktree", "prune"]);
    let added = process::run("git", Some(checkout), &["worktree", "add", "-q", "--detach", &worktree.to_string_lossy(), commit])?;
    if !added.status.success() {
        return Err(failed(1, format!("git worktree add {} {commit}: {}", worktree.display(), String::from_utf8_lossy(&added.stderr).trim())));
    }
    let mut build = Command::new(north_base::platform::north_binary::north_binary());
    build
        .args(["capacity", "host", "run", "--class", "runtime", "--owner", OWNER, "--timeout-seconds", BUILD_TIMEOUT_SECONDS, "--"])
        .args(cargo_build::cargo_words(None))
        .current_dir(worktree)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_PROFILE_RELEASE_INCREMENTAL", "true")
        .stdin(Stdio::null());
    let log = BuildLog::open(layout, &format!("ahead-{tree}"))?;
    let wall = std::time::SystemTime::now();
    log.attach(&mut build)?;
    let status = build.status()?;
    if !status.success() || !cargo_build::cargo_ran(target, wall) {
        return Err(cargo_build::build_failure(tree, status, cargo_build::cargo_ran(target, wall), &log));
    }
    log.discard();
    let release = target.join("release");
    if !PROGRAMS.iter().all(|program| release.join(program).is_file()) {
        return Err(failed(DEFERRED, format!("the release build of tree {} produced no whole runtime", layout::short(tree))));
    }
    Ok(release)
}

/// Writes the artifact whole: its programs, the commit and time they were built from, then the directory a promotion
/// finds by tree hash. A promotion reading it never sees a half-written one.
fn keep(tree: &str, commit: &str, release: &Path, artifacts: &Path) {
    let staging = artifacts.join(format!("{tree}.part"));
    let _ = fs::remove_dir_all(&staging);
    if fs::create_dir_all(&staging).is_err() {
        return;
    }
    for program in PROGRAMS {
        if layout::install(&release.join(program), &staging.join(program)).is_err() {
            let _ = fs::remove_dir_all(&staging);
            return;
        }
    }
    let _ = fs::write(staging.join(BUILT_FROM), format!("{commit}\n"));
    let _ = fs::write(staging.join(BUILT_AT), format!("{}\n", pin::now()));
    let _ = fs::remove_dir_all(artifacts.join(tree));
    if fs::rename(&staging, artifacts.join(tree)).is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
}

fn prune(artifacts: &Path) {
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(artifacts)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    kept.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in kept.into_iter().skip(KEPT) {
        println!("north runtime build-ahead: dropped the artifact for tree {}", path.file_name().unwrap_or_default().to_string_lossy());
        let _ = fs::remove_dir_all(&path);
    }
}

/// Takes the one build-ahead slot, waiting behind an older request: a lane's artifact is worth a later build, and a
/// request that gave up would leave its tree unbuilt with no later request for it.
fn take(lock: &Path) -> Option<File> {
    let file = File::create(lock).ok()?;
    if flock(&file, FlockOperation::NonBlockingLockExclusive).is_ok() {
        return Some(file);
    }
    println!("north runtime build-ahead: waiting for the release build ahead already running");
    flock(&file, FlockOperation::LockExclusive).ok().map(|()| file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(layout: &Layout, tree: &str) -> PathBuf {
        let artifact = layout.artifacts().join(tree);
        fs::create_dir_all(&artifact).unwrap();
        for program in PROGRAMS {
            fs::write(artifact.join(program), format!("{program} for {tree}\n")).unwrap();
        }
        artifact
    }

    #[test]
    fn a_ready_tree_is_adopted_with_no_build_and_only_the_tree_its_commit_carries_matches() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_path_buf() };
        let checkout = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(checkout.path()).args(args).status().unwrap().success());
        git(&["init", "-q"]);
        fs::write(checkout.path().join("a"), "a\n").unwrap();
        git(&["add", "a"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a"]);
        let tree = tree_of(checkout.path(), "HEAD").unwrap();
        assert!(ready_for(&layout, checkout.path(), "HEAD").is_none(), "no artifact is read before one is kept");
        artifact(&layout, &tree);
        assert_eq!(ready_for(&layout, checkout.path(), "HEAD").as_deref(), Some(tree.as_str()), "the promotion finds the artifact by the tree its landed commit carries");
        let runtime = layout.runtime(&"a".repeat(40));
        assert!(super::adopt(&layout, &runtime, &tree, &"a".repeat(40)));
        assert!(layout::has_programs(&runtime) && runtime.join(BUILT_HERE).is_file(), "the promotion installs the artifact as a whole runtime");
        assert!(pin::read(&layout.state).iter().any(|event| matches!(event, Event::ArtifactReady { sha, seconds: 0, .. } if sha == &"a".repeat(40))), "the promotion journals artifact-ready at 0 s");
        assert_eq!(layout::built_from_own_commit(&runtime), Ok(()), "the artifact's install is stamped for the landed commit");
        assert!(!super::adopt(&layout, &layout.runtime(&"b".repeat(40)), &"c".repeat(40), &"b".repeat(40)), "a tree with no artifact builds as before");
    }
}
