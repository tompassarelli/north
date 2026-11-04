use super::{CommitId, Land, RUN, VERIFY, WRONG_TREE, bundle, job_ref};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

fn remote_run(command: &str, flake: bool) -> String {
    remote_run_landing(command, flake, "")
}

fn remote_run_landing(command: &str, flake: bool, landing: &str) -> String {
    let slot = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    if flake {
        fs::write(slot.path().join("flake.nix"), "{}").unwrap();
    }
    let nix = bin.path().join("nix");
    fs::write(&nix, "#!/bin/sh\necho \"nix $*\"\n").unwrap();
    fs::set_permissions(&nix, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let output = Command::new("sh")
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h landing={}\n{RUN}", shell(slot.path()), shell(Path::new(command)), landing)])
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().replace(&slot.path().display().to_string(), "SLOT")
}

fn shell(word: &Path) -> String {
    super::quote(&word.display().to_string())
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn repository_with_two_commits() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "first"]);
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "second"]);
    let second = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, first, second)
}

fn verify(slot: &Path, head: &str) -> Output {
    let script = VERIFY.replace("@WRONG_TREE@", &WRONG_TREE.to_string());
    Command::new("sh").args(["-c", &format!("set -eu\nslot={} head={}\n{script}", shell(slot), super::quote(head))]).output().unwrap()
}

#[test]
fn a_checkout_with_a_flake_runs_the_command_inside_its_dev_shell() {
    assert_eq!(remote_run("cargo --version", true), "nix develop SLOT -c bash -euo pipefail -c cargo --version\n");
}

#[test]
fn a_checkout_without_a_flake_runs_the_command_plainly() {
    assert_eq!(remote_run("echo plain", false), "plain\n");
}

#[test]
fn the_landing_marker_reaches_the_command_only_when_the_caller_sent_it() {
    assert_eq!(remote_run_landing("echo \"[${SAFE_PUSH_LANDING-}]\"", false, "1"), "[1]\n");
    assert_eq!(remote_run_landing("echo \"[${SAFE_PUSH_LANDING-}]\"", false, ""), "[]\n");
}

#[test]
fn a_command_that_already_enters_a_dev_shell_is_not_wrapped_again() {
    assert_eq!(remote_run("nix develop -c cargo --version", true), "nix develop -c cargo --version\n");
}

#[test]
fn a_raw_name_reaches_land_only_as_the_full_commit_it_resolves_to_and_an_unknown_name_is_refused() {
    let (dir, _, second) = repository_with_two_commits();
    let head = CommitId::resolve(dir.path(), "HEAD").unwrap();
    assert_eq!(head.as_str(), second);
    assert_eq!(head.as_str().len(), 40);
    assert!(CommitId::resolve(dir.path(), "no-such-ref").is_err());
    assert!(CommitId::resolve(dir.path(), "").is_err());
}

#[test]
fn a_remote_checkout_that_ends_on_another_commit_is_refused_before_the_command_runs() {
    let (dir, first, second) = repository_with_two_commits();
    let refused = verify(dir.path(), &first);
    assert_eq!(refused.status.code(), Some(i32::from(WRONG_TREE)));
    assert!(refused.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&refused.stderr), format!("offload: refusing to run: {} is on {second}, not the shipped {first}\n", dir.path().display()));
    let ran = verify(dir.path(), &second);
    assert!(ran.status.success());
    assert_eq!(String::from_utf8(ran.stdout).unwrap().lines().next(), Some(format!("offload: running {second} in {}", dir.path().display()).as_str()));
}

#[test]
fn two_concurrent_jobs_at_the_same_head_bundle_under_distinct_refs() {
    let (dir, first, _) = repository_with_two_commits();
    let head = CommitId::resolve(dir.path(), "HEAD").unwrap();
    let base = CommitId::resolve(dir.path(), &first).unwrap();
    let references: Vec<String> = (0..2).map(|_| job_ref(&head)).collect();
    assert_ne!(references[0], references[1]);
    let heads: Vec<String> = std::thread::scope(|scope| {
        let jobs: Vec<_> = references
            .iter()
            .map(|reference| {
                let land = Land { ssh: "", checkout: dir.path(), base: Some(base.clone()), head: head.clone(), timeout_seconds: 1, command: "" };
                let checkout = dir.path();
                scope.spawn(move || {
                    let file = bundle(&land, reference).unwrap();
                    let listed = Command::new("git").current_dir(checkout).args(["bundle", "list-heads", "-"]).stdin(file).output().unwrap();
                    String::from_utf8(listed.stdout).unwrap().trim().to_owned()
                })
            })
            .collect();
        jobs.into_iter().map(|job| job.join().unwrap()).collect()
    });
    for (listed, reference) in heads.iter().zip(&references) {
        assert_eq!(listed, &format!("{} {reference}", head.as_str()));
        assert!(reference.starts_with(&format!("refs/land-offload/{}-", head.as_str())));
    }
    assert_eq!(git(dir.path(), &["for-each-ref", "refs/land-offload/"]), "");
}
