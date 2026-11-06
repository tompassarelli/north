use super::{CommitId, Land, VERIFY, WRONG_TREE, bundle, carried, job_ref, run_part};
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
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h carried={}\n{}", shell(slot.path()), shell(Path::new(command)), super::quote(&carried_from(caller)), run_part())])
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
fn a_remote_job_runs_only_on_the_shipped_commit_and_reads_each_checkout_beside_it_pinned_to_the_commit_it_is_on_here() {
    let (dir, first, second) = repository_with_two_commits();
    let refused = verify(dir.path(), &first);
    assert_eq!(refused.status.code(), Some(i32::from(WRONG_TREE)));
    assert!(refused.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&refused.stderr), format!("offload: refusing to run: {} is on {second}, not the shipped {first}\n", dir.path().display()));
    let ran = verify(dir.path(), &second);
    assert!(ran.status.success());
    assert_eq!(String::from_utf8(ran.stdout).unwrap().lines().next(), Some(format!("offload: running {second} in {}", dir.path().display()).as_str()));
    let command = "check-owners --north-root $HOME/code/north/main --south-root $HOME/code/south/main --nixos-root ~/code/nixos-config/main/agents $HOME/code/gone/main $HOME/code/south/mainline";
    let head_of = |path: &Path| path.ends_with("code/south/main").then(|| CommitId(first.clone())).or_else(|| path.ends_with("code/nixos-config/main").then(|| CommitId("b".repeat(40))));
    let pinned = super::beside::pin(command, "code/north/main", Path::new("/h"), head_of);
    assert_eq!(pinned.command, "check-owners --north-root $HOME/code/north/main --south-root $NORTH_OFFLOAD_BESIDE/south --nixos-root $NORTH_OFFLOAD_BESIDE/nixos-config/agents $HOME/code/gone/main $HOME/code/south/mainline");
    assert_eq!(pinned.pins, format!("south={first} nixos-config={}", "b".repeat(40)));
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("code/south")).unwrap();
    git(dir.path(), &["clone", "-q", &dir.path().display().to_string(), &home.path().join("code/south/main").display().to_string()]);
    let script = format!("set -eu\nroot={} s=slot pins={}\n{}git -C \"$NORTH_OFFLOAD_BESIDE/south\" rev-parse HEAD\n", shell(&home.path().join("root")), super::quote(&format!("south={first}")), super::beside_part());
    let read = Command::new("sh").args(["-c", &script]).env("HOME", home.path()).output().unwrap();
    assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stderr));
    assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), first);
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
    let script = super::REMOTE.replace("@ROOT@", super::OFFLOAD_ROOT).replace("@LOCK_WAIT@", "0").replace("@SLOTS@", "1").replace("@SLOT_PORTS@", "");
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
    let script = super::remote_script("code-p-main");
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

struct Queued {
    label: &'static str,
    name: &'static str,
    carried: &'static str,
    red_fix: &'static str,
    pause: u32,
}

/// Starts each job against one host slot, 0.9 s apart so they queue in the order given, and returns the order they started and ended in.
fn order_through_one_slot(jobs: &[Queued]) -> Vec<String> {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let children: Vec<std::process::Child> = jobs
        .iter()
        .map(|job| {
            let script = super::remote_script(job.name);
            let command = format!("echo start-{label} >>{log}; sleep {pause}; echo end-{label} >>{log}", label = job.label, pause = job.pause, log = log.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", job.name, &command, "refs/land-offload/job", job.carried, "", "", "0", job.red_fix])
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
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect()
}

#[test]
fn a_red_fix_land_check_then_any_land_check_takes_the_first_freed_slot_ahead_of_earlier_queued_worker_builds() {
    let landing = "SAFE_PUSH_LANDING='1'";
    let order = order_through_one_slot(&[
        Queued { label: "holder", name: "code-p-main", carried: "", red_fix: "", pause: 3 },
        Queued { label: "worker", name: "code-p-main", carried: "", red_fix: "", pause: 1 },
        Queued { label: "land", name: "code-p-main", carried: landing, red_fix: "", pause: 1 },
        Queued { label: "red-fix", name: "code-p-main", carried: landing, red_fix: "1", pause: 1 },
    ]);
    assert_eq!(order, ["start-holder", "end-holder", "start-red-fix", "end-red-fix", "start-land", "end-land", "start-worker", "end-worker"], "each queued after the worker build and still took the slot first, the red fix ahead of the land check");
}

#[test]
fn repositories_with_jobs_queued_for_one_slot_take_it_in_turn_instead_of_one_draining_first() {
    let job = |label, name, pause| Queued { label, name, carried: "", red_fix: "", pause };
    let order = order_through_one_slot(&[job("p-holder", "code-p-main", 6), job("p-2", "code-p-main", 1), job("p-3", "code-p-main", 1), job("q-1", "code-q-main", 1), job("q-2", "code-q-main", 1)]);
    assert_eq!(order, ["start-p-holder", "end-p-holder", "start-q-1", "end-q-1", "start-p-2", "end-p-2", "start-q-2", "end-q-2", "start-p-3", "end-p-3"], "q queued behind both of p's jobs and still ran before p's second");
}

#[test]
fn a_dead_job_leaves_no_ticket_that_blocks_the_queue() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let queue = home.path().join(".cache/land-offload/queue");
    fs::create_dir_all(&queue).unwrap();
    fs::write(queue.join("1-1000000000000000000-1"), "").unwrap();
    let script = super::remote_script("code-p-main");
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

#[test]
fn a_queued_job_reports_its_wait_and_runs_with_its_slot_as_the_slot_marker_and_the_sccache_base() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    crate::foundation::shim::write_script(&bin.path().join("nproc"), "echo 4\n");
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let seen = home.path().join("seen");
    let script = super::remote_script("code-p-main");
    let job = |command: String, lander: &str| {
        Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "SAFE_PUSH_LAND_CPUS='6'", "", "", "0", "", "", "", lander])
            .env("HOME", home.path())
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", "1")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(crate::capacity::land_claim::OFFLOAD_SLOT_ENV)
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let holder = job("sleep 2".to_owned(), "");
    std::thread::sleep(std::time::Duration::from_millis(500));
    let queued = job(format!("printf '%s %s %s %s %s' \"${}\" \"$SCCACHE_BASEDIRS\" \"$SCCACHE_SERVER_PORT\" \"$CARGO_BUILD_JOBS $RUST_TEST_THREADS $NEXTEST_TEST_THREADS\" \"$SAFE_PUSH_LAND_CPUS\" >{}", crate::capacity::land_claim::OFFLOAD_SLOT_ENV, seen.display()), "");
    let lander_seen = home.path().join("lander-seen");
    let lander = job(format!("printf '%s' \"${}\" >{}", crate::capacity::land_claim::OFFLOAD_SLOT_ENV, lander_seen.display()), "1").wait_with_output().unwrap();
    assert!(lander.status.success(), "{}", String::from_utf8_lossy(&lander.stderr));
    assert!(!String::from_utf8_lossy(&lander.stderr).contains(super::WAITED), "a lander job takes its reserved slot without waiting behind the job cap");
    assert_eq!(fs::read_to_string(&lander_seen).unwrap(), home.path().join(".cache/land-offload/code-p-main-5").display().to_string());
    let queued = queued.wait_with_output().unwrap();
    assert!(holder.wait_with_output().unwrap().status.success());
    assert!(queued.status.success(), "{}", String::from_utf8_lossy(&queued.stderr));
    let stderr = String::from_utf8_lossy(&queued.stderr);
    assert!(stderr.lines().any(|line| line.starts_with(super::WAITED) && line.ends_with("s in line for code-p-main")), "{stderr}");
    let slot = home.path().join(".cache/land-offload/code-p-main");
    assert_eq!(fs::read_to_string(&seen).unwrap(), format!("{0} {0} {1} 4 4 4 4", slot.display(), super::slot_ports("code-p-main")[0].1), "a slot job builds and tests on the host's cores divided by its job cap");
}

fn remote_reports(command: &str, marker: &str) -> (Option<u8>, Option<u8>, String) {
    let slot = tempfile::tempdir().unwrap();
    let script = format!("set -eu\nslot={} command={} limit=10 ran={}\n{}", shell(slot.path()), super::quote(command), super::quote(marker), run_part());
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let output = Command::new("sh").args(["-c", &script]).env("HOME", slot.path()).env("PATH", path).output().unwrap();
    let mut shown = Vec::new();
    let ran = super::relay(output.stdout.as_slice(), &mut shown, marker);
    (output.status.code().and_then(|code| u8::try_from(code).ok()), ran, String::from_utf8(shown).unwrap())
}

#[test]
fn an_offload_passes_only_when_the_remote_reports_its_command_ran_and_succeeded() {
    let marker = "north-offload-ran:job-1";
    let (exited, ran, shown) = remote_reports("printf partial; true", marker);
    assert_eq!((super::verdict(exited, ran), shown.as_str()), (Ok(0), "partial\n"));
    let (exited, ran, _) = remote_reports("false", marker);
    assert_eq!(super::verdict(exited, ran), Ok(1));
    let mut shown = Vec::new();
    let never_ran = super::relay(b"bash: --: invalid option\nUsage: bash [option] ...\n".as_slice(), &mut shown, marker);
    assert!(super::verdict(Some(0), never_ran).is_err(), "a remote that exits 0 without reporting its command passed");
    assert_eq!(String::from_utf8(shown).unwrap(), "bash: --: invalid option\nUsage: bash [option] ...\n");
}

#[test]
fn every_slot_of_a_checkout_serves_sccache_on_its_own_stable_port_in_range() {
    let ports = super::slot_ports("code-north-main");
    assert_eq!(ports, super::slot_ports("code-north-main"));
    assert_eq!(ports.iter().map(|(slot, _)| slot.as_str()).collect::<Vec<_>>(), ["code-north-main", "code-north-main-1", "code-north-main-2", "code-north-main-3", "code-north-main-4", "code-north-main-5"]);
    let mut numbers: Vec<u32> = ports.iter().map(|(_, port)| *port).collect();
    assert!(numbers.iter().all(|port| (20000..40000).contains(port)), "{numbers:?}");
    numbers.dedup();
    assert_eq!(numbers.len(), super::SLOTS as usize);
}
