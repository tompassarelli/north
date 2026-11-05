use super::{CommitId, Land, RUN, VERIFY, WRONG_TREE, bundle, carried, job_ref};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn carried_from(caller: &[(&str, &str)]) -> String {
    let land = Land { ssh: "", checkout: Path::new("."), base: Some(CommitId("b".to_owned())), head: CommitId("h".to_owned()), timeout_seconds: 10, command: "" };
    carried(&land, |name| caller.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned()))
}

const FLAKE_WITHOUT_DEV_SHELL: &str = "{ outputs = { checks = 1; }; }";

const NIX_SHIM: &str = "echo \"nix $*\"\n";

const NORTH_SHIM: &str = r#"case "$1 $2" in
  "capacity deadline") shift; while [ "$1" != -- ]; do shift; done; shift; exec "$@" ;;
  "dev-shell run")
    dir=$4
    shift 5
    if grep -qs devShells "$dir/flake.nix"; then echo "dev-shell $dir $*"; exit 0; fi
    exec "$@" ;;
esac
exit 64
"#;

fn remote_run_flake(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>) -> String {
    let slot = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    if let Some(text) = flake {
        fs::write(slot.path().join("flake.nix"), text).unwrap();
    }
    let nix = bin.path().join("nix");
    crate::foundation::shim::write_script(&nix, NIX_SHIM);
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let fresh_home = tempfile::tempdir().unwrap();
    let output = Command::new("sh")
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h carried={}\n{RUN}", shell(slot.path()), shell(Path::new(command)), super::quote(&carried_from(caller)))])
        .env("PATH", path)
        .env_remove("SAFE_PUSH_LAND_BASE")
        .env_remove("SAFE_PUSH_LAND_HEAD")
        .env_remove("SAFE_PUSH_LAND_REF")
        .env_remove("SAFE_PUSH_LANDING")
        .env_remove("SAFE_PUSH_LAND_CPUS")
        .env_remove("SAFE_PUSH_LAND_PACKAGES")
        .env_remove("SAFE_PUSH_LAND_IT")
        .env_remove("SAFE_PUSH_LAND_SELECT_PATHS")
        .env("HOME", home.unwrap_or(fresh_home.path()))
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
fn a_flake_without_a_dev_shell_runs_the_command_plainly() {
    assert_eq!(remote_run_flake("echo plain", Some(FLAKE_WITHOUT_DEV_SHELL), &[], None), "plain\n");
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
fn a_job_whose_base_is_its_head_bundles_the_head_over_its_parent() {
    let (dir, _, _) = repository_with_two_commits();
    let head = CommitId::resolve(dir.path(), "HEAD").unwrap();
    let reference = job_ref(&head);
    let land = Land { ssh: "", checkout: dir.path(), base: Some(head.clone()), head: head.clone(), timeout_seconds: 1, command: "" };
    let file = bundle(&land, &reference).unwrap();
    let listed = Command::new("git").current_dir(dir.path()).args(["bundle", "list-heads", "-"]).stdin(file).output().unwrap();
    assert_eq!(String::from_utf8(listed.stdout).unwrap().trim(), format!("{} {reference}", head.as_str()));
}


fn host_with_a_shipped_bundle() -> (tempfile::TempDir, String, std::path::PathBuf) {
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
    (home, head, bundle)
}

#[test]
fn offload_jobs_past_the_host_cap_wait_their_turn_in_arrival_order() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let script = super::remote_script("0.2");
    let jobs: Vec<std::process::Child> = ["a", "b", "c", "d"]
        .iter()
        .map(|label| {
            let command = format!("echo start-{label} >>{log}; sleep 0.5; echo end-{label} >>{log}", log = log.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "0"])
                .env("HOME", home.path())
                .env("PATH", &path)
                .env("NORTH_OFFLOAD_MAX_JOBS", "1")
                .env_remove("NORTH_OFFLOAD_ROOT")
                .stdin(fs::File::open(&bundle).unwrap())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(900));
            child
        })
        .collect();
    for mut job in jobs {
        assert!(job.wait().unwrap().success());
    }
    let order: Vec<String> = fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
    assert_eq!(order, ["start-a", "end-a", "start-b", "end-b", "start-c", "end-c", "start-d", "end-d"]);
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "every job left the queue");
}

#[test]
fn a_dead_job_leaves_no_ticket_that_blocks_the_queue() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let queue = home.path().join(".cache/land-offload/queue");
    fs::create_dir_all(&queue).unwrap();
    fs::write(queue.join("1000000000000000000-1"), "").unwrap();
    let script = super::remote_script("0.2");
    let output = Command::new("sh")
        .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", "", "0"])
        .env("HOME", home.path())
        .env("NORTH_OFFLOAD_MAX_JOBS", "1")
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(fs::File::open(&bundle).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_dir(&queue).unwrap().count(), 0);
}
