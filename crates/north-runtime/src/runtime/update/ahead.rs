//! Release runtimes built ahead of the landing they belong to: the lander asks for the release north of a queued
//! lane's candidate tree, a build host builds it, and the promotion installs it by the tree hash the landed commit
//! carries instead of building the same tree again after the push. An artifact is kept under
//! `runtime-artifacts/<tree>`: the runtime programs, the commit and the host they were built from and on, and when.
//! One request runs at a time, only when no artifact for that tree is ready yet, and none builds on this host.
use super::super::build_host;
use super::super::failure::{Failure, failed};
use super::super::layout::{self, BUILT_HERE, BUILT_ON, Layout, PROGRAMS};
use super::super::pin::{self, Event};
use super::super::remote;
use crate::land_offload::{self, BuildHost};
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const BUILT_AT: &str = "built-at";
const BUILD_LOCK: &str = "build.lock";
const KEPT: usize = 3;
const SEND: Duration = Duration::from_secs(120);

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
    match artifact.join(BUILT_ON).is_file() {
        true => fs::copy(artifact.join(BUILT_ON), runtime.join(BUILT_ON)).is_ok(),
        false => fs::write(runtime.join(BUILT_HERE), layout::short(target)).is_ok(),
    }
}

/// `north runtime build-ahead --tree TREE --commit COMMIT [--checkout DIR]`: the lander's request for the release
/// north a lane's candidate tree carries, kept where the promotion of that tree's landing finds it.
pub(in crate::runtime) fn run(layout: &Layout, flags: &[String]) -> Result<(), Failure> {
    let mut tree = None;
    let mut commit = None;
    let mut checkout = std::env::current_dir().map_err(|error| failed(64, format!("current directory: {error}")))?;
    let mut words = flags.iter();
    while let Some(flag) = words.next() {
        let value = words.next().cloned().ok_or_else(|| failed(64, format!("{flag} needs a value")))?;
        match flag.as_str() {
            "--tree" => tree = Some(value),
            "--commit" => commit = Some(value),
            "--checkout" => checkout = PathBuf::from(value),
            other => return Err(failed(64, format!("unknown build-ahead flag {other}"))),
        }
    }
    let (Some(tree), Some(commit)) = (tree, commit) else {
        return Err(failed(64, "build-ahead needs --tree TREE --commit COMMIT"));
    };
    build(layout, &checkout, &tree, &commit)
}

/// Has a build host build the release north for `tree` from `commit` in `checkout` and keeps it as the promotion's
/// artifact; a tree another request already fills, or one ready already, costs nothing.
pub(super) fn build(layout: &Layout, checkout: &Path, tree: &str, commit: &str) -> Result<(), Failure> {
    request(layout, tree, commit, land_offload::build_host_rooms, |host, commit, into| {
        send(checkout, host, commit)?;
        build_host::build_on(host, commit, into)
    })
}

/// Keeps what the freest build host built for `tree`. This host settles the fleet while people work on it, so it
/// never builds the release itself: with no build host answering, or the chosen one failing, nothing is kept and the
/// promotion of the landing has a build host build it.
fn request(layout: &Layout, tree: &str, commit: &str, hosts: impl FnOnce() -> Vec<BuildHost>, build_on: impl FnMut(&BuildHost, &str, &Path) -> Result<(), Failure>) -> Result<(), Failure> {
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
    let staging = artifacts.join(format!("{tree}.part"));
    let _ = fs::remove_dir_all(&staging);
    let taken = staging.join(commit);
    let kept = build_host::build(commit, &taken, hosts(), build_on).and_then(|built| keep(&taken, &artifacts.join(tree)).map(|()| built).map_err(|error| format!("could not keep the build under {}: {error}", artifacts.display())));
    let _ = fs::remove_dir_all(&staging);
    match kept {
        Ok(built) => {
            prune(&artifacts);
            println!("north runtime build-ahead: tree {} built on {} in {}s, ready for the promotion of {}", layout::short(tree), built.host, built.seconds, layout::short(commit));
        }
        Err(why) => println!("north runtime build-ahead: {why}; this host builds no release ahead, so the promotion of tree {} has a build host build it", layout::short(tree)),
    }
    Ok(())
}

/// The ref a candidate commit is sent to a build host under: it has not landed, so the host's `north runtime build`
/// takes this ref in place of origin/main.
fn candidate_ref(commit: &str) -> String {
    format!("refs/build-ahead/{commit}")
}

fn send(checkout: &Path, host: &BuildHost, commit: &str) -> Result<(), Failure> {
    let mut push = Command::new("git");
    push.arg("-C").arg(checkout).args(["push", "-q", "--no-verify"]).arg(format!("{}:{}", host.ssh, super::CHECKOUT)).arg(format!("{commit}:{}", candidate_ref(commit))).env("GIT_SSH_COMMAND", format!("{} -o BatchMode=yes -o ConnectTimeout=20", remote::OWN_RSH));
    remote::run(&host.name, "sending the candidate commit", &mut push, SEND)
}

/// On a build host: whether `commit` was sent here as a lander's candidate.
pub(super) fn sent(checkout: &Path, commit: &str) -> bool {
    process::run("git", Some(checkout), &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", candidate_ref(commit))]).is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == commit)
}

/// On a build host: drops the ref a candidate was sent under once its build ended, so the refs never pile up.
pub(super) fn forget(checkout: &Path, commit: &str) {
    let _ = process::run("git", Some(checkout), &["update-ref", "-d", &candidate_ref(commit)]);
}

/// Completes the artifact a build host's programs were taken into, then gives it the name a promotion finds by tree
/// hash. A promotion reading it never sees a half-written one.
fn keep(taken: &Path, artifact: &Path) -> std::io::Result<()> {
    fs::write(taken.join(BUILT_AT), format!("{}\n", pin::now()))?;
    let _ = fs::remove_dir_all(artifact);
    fs::rename(taken, artifact)
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
        let journal = fs::read_to_string(layout.state.join(pin::JOURNAL)).unwrap();
        assert!(journal.contains("\"event\":\"artifact-ready\"") && journal.contains("\"seconds\":0"), "the ledger line is the one the box's jq reads");
        assert_eq!(layout::built_from_own_commit(&runtime), Ok(()), "the artifact's install is stamped for the landed commit");
        assert!(!super::adopt(&layout, &layout.runtime(&"b".repeat(40)), &"c".repeat(40), &"b".repeat(40)), "a tree with no artifact builds as before");
    }

    fn host(name: &str, free_cpus: Option<f64>) -> BuildHost {
        BuildHost { name: name.to_owned(), ssh: format!("tom@{name}"), free_cpus }
    }

    #[test]
    fn build_ahead_never_runs_cargo_on_the_settling_host() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_path_buf() };
        let (tree, commit) = ("c".repeat(40), "d".repeat(40));
        let unbuilt = |_: &BuildHost, _: &str, _: &Path| -> Result<(), Failure> { panic!("no build host answered") };
        assert!(request(&layout, &tree, &commit, || vec![host("cpx62", None), host("gex45", None)], unbuilt).is_ok(), "with no build host answering the request ends clean");
        let left: Vec<_> = fs::read_dir(layout.artifacts()).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(left, [BUILD_LOCK], "and it built nothing here: the promotion has a build host build the tree");

        let mut asked = Vec::new();
        let built = |host: &BuildHost, commit: &str, into: &Path| -> Result<(), Failure> {
            asked.push(format!("{} {commit}", host.name));
            fs::create_dir_all(into)?;
            for program in PROGRAMS {
                fs::write(into.join(program), format!("{program} built on {}", host.name))?;
            }
            Ok(layout::stamp(into, commit)?)
        };
        request(&layout, &tree, &commit, || vec![host("gex45", Some(4.0)), host("cpx62", Some(12.0))], built).unwrap();
        assert_eq!(asked, [format!("cpx62 {commit}")], "the candidate commit is built on the build host with the most free cores");
        let artifact = ready(&layout, &tree).expect("the build host's release is kept by the candidate's tree");
        assert_eq!(fs::read_to_string(artifact.join("north")).unwrap(), "north built on cpx62");
        let landed = "e".repeat(40);
        let runtime = layout.runtime(&landed);
        assert!(super::adopt(&layout, &runtime, &tree, &landed));
        assert!(fs::read_to_string(runtime.join(BUILT_ON)).unwrap().starts_with("cpx62 ") && !runtime.join(BUILT_HERE).exists(), "the promoted runtime says which host built it");
        assert_eq!(layout::built_from_own_commit(&runtime), Ok(()));
    }
}
