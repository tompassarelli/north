use super::{CommitId, Land, RUN, VERIFY, WRONG_TREE, bundle, job_ref, remote_checkout};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn remote_run(command: &str, flake: bool) -> String {
    remote_run_landing(command, flake, "")
}

fn remote_run_landing(command: &str, flake: bool, landing: &str) -> String {
    remote_run_with(command, flake, landing, "", None)
}

fn remote_run_with(command: &str, flake: bool, landing: &str, landref: &str, home: Option<&Path>) -> String {
    remote_run_flake(command, flake.then_some(FLAKE_WITH_DEV_SHELL), landing, landref, home)
}

const FLAKE_WITH_DEV_SHELL: &str = "{ outputs = { devShells = 1; }; }";
const FLAKE_WITHOUT_DEV_SHELL: &str = "{ outputs = { checks = 1; }; }";

const NIX_SHIM: &str = r#"case "$1" in
  eval)
    case "$*" in *currentSystem*) echo x86_64-linux; exit 0 ;; esac
    for last; do :; done
    grep -q devShells "${last%%#*}/flake.nix"
    exit $? ;;
esac
echo "nix $*"
"#;

fn remote_run_flake(command: &str, flake: Option<&str>, landing: &str, landref: &str, home: Option<&Path>) -> String {
    let slot = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    if let Some(text) = flake {
        fs::write(slot.path().join("flake.nix"), text).unwrap();
    }
    let nix = bin.path().join("nix");
    crate::foundation::shim::write_script(&nix, NIX_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let output = Command::new("sh")
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h landing={} landref={}\n{RUN}", shell(slot.path()), shell(Path::new(command)), landing, super::quote(landref))])
        .env("PATH", path)
        .env_remove("SAFE_PUSH_LAND_BASE")
        .env_remove("SAFE_PUSH_LAND_HEAD")
        .env_remove("SAFE_PUSH_LAND_REF")
        .env_remove("SAFE_PUSH_LANDING")
        .envs(home.map(|home| ("HOME", home)))
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

fn reap_script(root: &Path, owner: &str) -> Output {
    let script = super::REAP.replace("@ROOT@", &shell(root));
    Command::new("sh").args(["-c", &script, "offload-reap", owner]).output().unwrap()
}

fn offload_run(root: &Path, name: &str, owner: &str) -> std::process::Child {
    let child = Command::new("setsid").args(["-w", "sh", "-c", "sleep 60; sleep 1", "offload"]).spawn().unwrap();
    fs::write(root.join(format!("{name}.owner")), format!("{owner}\n")).unwrap();
    fs::write(root.join(format!("{name}.pgid")), format!("{}\n", child.id())).unwrap();
    fs::write(root.join(format!("{name}.holder")), "abc since 00:00:00\n").unwrap();
    child
}

#[test]
fn reaping_an_owner_kills_its_remote_runs_and_leaves_every_other_run_alone() {
    let root = tempfile::tempdir().unwrap();
    let mut mine = offload_run(root.path(), "north", "north-land");
    let mut other = offload_run(root.path(), "north-1", "safe-push-lane");
    let output = reap_script(root.path(), "north-land");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("reaped north (process group"));
    assert!(mine.wait().unwrap().code().is_none(), "the owner's run died on a signal");
    assert!(other.try_wait().unwrap().is_none(), "another owner's run is still running");
    assert!(!root.path().join("north.owner").exists() && !root.path().join("north.pgid").exists() && !root.path().join("north.holder").exists());
    assert!(root.path().join("north-1.owner").exists());
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn reaping_forgets_a_run_whose_process_is_already_gone() {
    let root = tempfile::tempdir().unwrap();
    let mut gone = Command::new("true").spawn().unwrap();
    gone.wait().unwrap();
    fs::write(root.path().join("north.owner"), "north-land\n").unwrap();
    fs::write(root.path().join("north.pgid"), format!("{}\n", gone.id())).unwrap();
    assert!(reap_script(root.path(), "north-land").status.success());
    assert!(!root.path().join("north.owner").exists());
}

#[test]
fn the_remote_script_records_its_owner_and_process_group_and_removes_them_on_exit() {
    assert!(super::REMOTE.contains("owner=${10:-}"));
    assert!(super::REMOTE.contains(r#"printf '%s\n' "$owner" >"$root/$s.owner""#));
    assert!(super::REMOTE.contains(r#"printf '%s\n' "$$" >"$root/$s.pgid""#));
    assert!(super::REMOTE.contains(r#"rm -f "$bundle" "$root/$s.owner" "$root/$s.pgid""#));
}

#[test]
fn a_flake_without_a_dev_shell_runs_the_command_plainly() {
    assert_eq!(remote_run_flake("echo plain", Some(FLAKE_WITHOUT_DEV_SHELL), "", "", None), "plain\n");
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

#[test]
fn a_landing_run_sees_the_offloads_base_head_and_published_ref_exactly_as_a_local_land_check_does() {
    let seen = remote_run_with("echo \"$SAFE_PUSH_LAND_BASE $SAFE_PUSH_LAND_HEAD [$SAFE_PUSH_LAND_REF] $SAFE_PUSH_LANDING\"", false, "1", "land-check/lane-1", None);
    assert_eq!(seen, "b h [land-check/lane-1] 1\n");
    let unpublished = remote_run_with("echo \"[$SAFE_PUSH_LAND_REF]\"", false, "1", "", None);
    assert_eq!(unpublished, "[]\n");
}

#[test]
fn a_run_that_is_not_a_landing_check_carries_no_landing_context() {
    let seen = remote_run_with("echo \"[${SAFE_PUSH_LAND_BASE-}][${SAFE_PUSH_LAND_HEAD-}][${SAFE_PUSH_LAND_REF-}][${SAFE_PUSH_LANDING-}]\"", false, "", "stale", None);
    assert_eq!(seen, "[][][][]\n");
}

#[test]
fn the_hosts_runtime_bin_directory_leads_the_path_before_the_command_runs() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/bin")).unwrap();
    let path = remote_run_with("echo \"$PATH\"", false, "", "", Some(home.path()));
    assert!(path.starts_with(&format!("{}/.local/bin:", home.path().display())), "{path}");
}

#[test]
fn a_host_without_a_runtime_directory_keeps_its_path() {
    let home = tempfile::tempdir().unwrap();
    let path = remote_run_with("echo \"$PATH\"", false, "", "", Some(home.path()));
    assert!(!path.contains(&home.path().display().to_string()), "{path}");
}

#[test]
fn shipping_a_job_runs_no_git_maintenance_that_could_prune_a_worktree_while_git_creates_it() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("code/p/main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "maintenance.autoDetach", "false"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["update-ref", "refs/land-offload/job", &head]);
    let bundle = home.path().join("job.bundle");
    git(&repo, &["bundle", "create", "-q", bundle.to_str().unwrap(), "refs/land-offload/job"]);
    git(&repo, &["update-ref", "-d", "refs/land-offload/job"]);
    let half_made = repo.join(".git/worktrees/half-made");
    fs::create_dir_all(&half_made).unwrap();
    let script = super::REMOTE.replace("@ROOT@", super::OFFLOAD_ROOT).replace("@LOCK_WAIT@", "0").replace("@SLOTS@", "1");
    let output = Command::new("sh")
        .args(["-c", &script, "offload", "code/p/main", "", &head, "10", "code-p-main", "true", "refs/land-offload/job"])
        .env("HOME", home.path())
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(fs::File::open(&bundle).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(git(&home.path().join(".cache/land-offload/code-p-main"), &["rev-parse", "HEAD"]), head);
    assert!(half_made.is_dir(), "a fetch's auto maintenance pruned a worktree entry that has no gitdir yet, as it would one git worktree add is still creating");
}

#[test]
fn a_project_under_code_ships_to_its_main_checkout_whatever_this_host_keeps_its_git_directory_in() {
    let home = Path::new("/home/u");
    assert_eq!(remote_checkout(Path::new("/home/u/code/wisp/main"), home).unwrap(), "code/wisp/main");
    assert_eq!(remote_checkout(Path::new("/home/u/code/wisp/worktrees/extract-20261005"), home).unwrap(), "code/wisp/main");
    assert_eq!(remote_checkout(Path::new("/home/u/src/tool"), home).unwrap(), "src/tool");
    assert!(remote_checkout(Path::new("/srv/repo"), home).is_err());
}
