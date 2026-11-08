use super::script::{VERIFY, run_part};
use super::{CommitId, Land, WRONG_TREE, bundle, carried, job_ref};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn carried_from(caller: &[(&str, &str)]) -> String {
    let land = Land { ssh: "", checkout: Path::new("."), base: Some(CommitId("b".to_owned())), head: CommitId("h".to_owned()), timeout_seconds: 10, command: "", class: super::SlotClass::LandCheck };
    carried(&land, |name| caller.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned()))
}

const FLAKE_WITHOUT_DEV_SHELL: &str = "{ outputs = { checks = 1; }; }";

const NIX_SHIM: &str = "echo \"nix $*\"\n";

const NORTH_SHIM: &str = r#"case "$1 $2" in
  "capacity deadline") shift; while [ "$1" != -- ]; do shift; done; shift; exec "$@" ;;
  "capacity host")
    shift 2
    echo "lease $*" | sed 's/ -- .*//' >&2
    while [ "$1" != -- ]; do shift; done
    shift
    if grep -qs devShells flake.nix; then echo "dev-shell $PWD $*"; exit 0; fi
    exec "$@" ;;
esac
exit 64
"#;

fn remote_run_flake(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>) -> String {
    remote_run(command, flake, caller, home, "").0
}

fn remote_run(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>, prelude: &str) -> (String, String) {
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
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h carried={}\n{prelude}\n{}", shell(slot.path()), shell(Path::new(command)), super::quote(&carried_from(caller)), run_part())])
        .env("PATH", path)
        .env_remove("SAFE_PUSH_LAND_BASE")
        .env_remove("SAFE_PUSH_LAND_HEAD")
        .env_remove("SAFE_PUSH_LAND_REF")
        .env_remove("SAFE_PUSH_LANDING")
        .env_remove("SAFE_PUSH_LAND_CPUS")
        .env_remove("SAFE_PUSH_LAND_PACKAGES")
        .env_remove("SAFE_PUSH_LAND_IT")
        .env_remove("SAFE_PUSH_LAND_SELECT_PATHS")
        .env_remove("SAFE_PUSH_LAND_TIPS")
        .env("HOME", home.unwrap_or(fresh_home.path()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let shown = slot.path().display().to_string();
    (String::from_utf8(output.stdout).unwrap().replace(&shown, "SLOT"), String::from_utf8(output.stderr).unwrap().replace(&shown, "SLOT"))
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
    let group = rustix::process::Pid::from_raw(other.id() as i32).unwrap();
    rustix::process::kill_process_group(group, rustix::process::Signal::Kill).unwrap();
    other.wait().unwrap();
}

#[test]
fn a_flake_without_a_dev_shell_runs_the_command_plainly() {
    assert_eq!(remote_run_flake("echo plain", Some(FLAKE_WITHOUT_DEV_SHELL), &[], None), "plain\n");
    let (stdout, stderr) = remote_run("printf '%s' \"$SAFE_PUSH_LAND_CPUS\"", None, &[("SAFE_PUSH_LANDING", "1")], None, "cores=16 cap=4");
    assert_eq!(stdout, "4", "a slot command's lease is charged its share of the host's cores");
    assert!(stderr.lines().any(|line| line == "lease run --class heavy --owner land-check --timeout-seconds 10"), "{stderr}");
    let (_, stderr) = remote_run("true", None, &[], None, "");
    assert!(stderr.lines().any(|line| line == "lease run --class heavy --owner land-offload --timeout-seconds 10"), "{stderr}");
    let job = super::job_mib();
    let cases = [(8, 4, 31_000, 2, 2, "ccx33 at 4 cores a land check: its heavy leases keep the CPU headroom, so no 4/5 cut leaves it one job"), (20, 3, 64_000, 6, 6, "gex45 at 3 cores a land check"), (48, 4, 192_000, 12, super::SLOTS, "a host with room for more jobs than one checkout has slots runs that many across its checkouts"), (8, 1, 3 * job, 3, 3, "memory caps a host whose land checks charge one core"), (2, 4, 64_000, 1, 1, "a host smaller than one land check still runs one")];
    for (cores, per, memory, host, expected, why) in cases {
        let meminfo = tempfile::NamedTempFile::new().unwrap();
        fs::write(meminfo.path(), format!("MemTotal:       {} kB\n", memory * 1024)).unwrap();
        let script = format!("per={per}\n{}echo \"$host_cap $cap\"", super::script::CAP.replace("@SLOTS@", &super::SLOTS.to_string()).replace("@JOB_MIB@", &job.to_string()));
        let output = Command::new("sh").args(["-c", &script]).env("AGENT_CAPACITY_CORES", cores.to_string()).env("AGENT_CAPACITY_MEMINFO", meminfo.path()).env_remove("NORTH_OFFLOAD_MAX_JOBS").output().unwrap();
        let shell: Vec<u32> = String::from_utf8_lossy(&output.stdout).split_whitespace().map(|word| word.parse().unwrap()).collect();
        assert_eq!((shell, super::host_job_cap(cores, per, memory)), (vec![host, expected], host), "{why}");
    }
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
    let mut slot_waits_ended = 0;
    super::relay(ran.stdout.as_slice(), &mut Vec::new(), "north-offload-ran:job", || slot_waits_ended += 1);
    assert_eq!(slot_waits_ended, 1, "the line the remote prints once it holds its slot ends the slot wait a deadline above the offload excuses");
    assert_eq!(String::from_utf8(ran.stdout).unwrap().lines().next(), Some(format!("offload: running {second} in {}", dir.path().display()).as_str()));
    let command = "check-owners --north-root $HOME/code/north/main --south-root $HOME/code/south/main --nixos-root ~/code/nixos-config/main/agents $HOME/code/gone/main $HOME/code/south/mainline";
    let head_of = |path: &Path| path.ends_with("code/south/main").then(|| CommitId(first.clone())).or_else(|| path.ends_with("code/nixos-config/main").then(|| CommitId("b".repeat(40))));
    let pinned = super::beside::pin(command, "code/north/main", Path::new("/h"), head_of);
    assert_eq!(pinned.command, "check-owners --north-root $HOME/code/north/main --south-root $NORTH_OFFLOAD_BESIDE/south --nixos-root $NORTH_OFFLOAD_BESIDE/nixos-config/agents $HOME/code/gone/main $HOME/code/south/mainline");
    assert_eq!(pinned.pins, format!("south={first} nixos-config={}", "b".repeat(40)));
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("code/south")).unwrap();
    git(dir.path(), &["clone", "-q", &dir.path().display().to_string(), &home.path().join("code/south/main").display().to_string()]);
    let script = format!("set -eu\nroot={} s=slot pins={}\n{}git -C \"$NORTH_OFFLOAD_BESIDE/south\" rev-parse HEAD\n", shell(&home.path().join("root")), super::quote(&format!("south={first}")), super::script::beside_part());
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
                let land = Land { ssh: "", checkout: dir.path(), base: Some(base.clone()), head: head.clone(), timeout_seconds: 1, command: "", class: super::SlotClass::LandCheck };
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
fn shipping_a_job_recovers_its_slots_stale_worktree_entry_and_prunes_no_other_entry_while_git_creates_it() {
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
    let slot = home.path().join(".cache/land-offload/code-p-main");
    git(&repo, &["worktree", "add", "-q", "--detach", slot.to_str().unwrap(), &head]);
    fs::remove_dir_all(&slot).unwrap();
    let script = super::script::slot_part().replace("@JOBS@", &super::script::jobs_part()).replace("@CAP@", super::script::CAP).replace("@ROOT@", super::OFFLOAD_ROOT).replace("@LOCK_WAIT@", "0").replace("@SLOTS@", "1").replace("@JOB_MIB@", "0").replace("@SLOT_PORTS@", "");
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
    let land = Land { ssh: "", checkout: dir.path(), base: Some(head.clone()), head: head.clone(), timeout_seconds: 1, command: "", class: super::SlotClass::LandCheck };
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

/// Blocks until `jobs` of the spawned scripts have either a ticket in the host queue or a start line in the order log.
fn wait_until_in_line(home: &Path, log: &Path, jobs: usize) {
    let counts = || {
        let queued = fs::read_dir(home.join(".cache/land-offload/queue")).map(|entries| entries.count()).unwrap_or(0);
        let started = fs::read_to_string(log).map(|text| text.lines().filter(|line| line.starts_with("start-")).count()).unwrap_or(0);
        (queued, started)
    };
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || counts().0 + counts().1 >= jobs), "only {} queued and {} started of {jobs} jobs", counts().0, counts().1);
}

#[test]
fn offload_jobs_past_the_host_cap_wait_their_turn_in_arrival_order() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let root = home.path().join(".cache/land-offload");
    fs::create_dir_all(&root).unwrap();
    let warming = fs::File::create(root.join(super::super::warm::LOCK)).unwrap();
    rustix::fs::flock(&warming, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let jobs: Vec<std::process::Child> = ["a", "b", "c", "d"]
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let release = home.path().join("release");
            let command = format!("echo start-{label} >>{log}; while [ ! -e {release} ]; do sleep 0.02; done; echo end-{label} >>{log}", log = log.display(), release = release.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "0"])
                .env("HOME", home.path())
                .env("PATH", &path)
                .env("NORTH_OFFLOAD_MAX_JOBS", "1")
                .env_remove("NORTH_OFFLOAD_ROOT")
                .stdin(fs::File::open(&bundle).unwrap())
                .stdout(std::process::Stdio::null())
                .stderr(fs::File::create(home.path().join(format!("{label}.err"))).unwrap())
                .spawn()
                .unwrap();
            wait_until_in_line(home.path(), &log, index + 1);
            if index == 0 {
                assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.contains("start-a"))), "the first job never started");
            }
            child
        })
        .collect();
    fs::write(home.path().join("release"), "").unwrap();
    for mut job in jobs {
        assert!(job.wait().unwrap().success());
    }
    let order: Vec<String> = fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
    assert_eq!(order, ["start-a", "end-a", "start-b", "end-b", "start-c", "end-c", "start-d", "end-d"]);
    let waiting = fs::read_to_string(home.path().join("b.err")).unwrap();
    let line = waiting.lines().find(|line| line.starts_with("offload: waiting for a slot")).unwrap_or_else(|| panic!("{waiting}"));
    let held = line.split_once("held by: ").map_or("", |(_, held)| held);
    assert!(line.contains("code-p-main runs 1 of 1 jobs at once, the host 1 of 1;") && held.starts_with(&format!("code-p-main.lock {} since ", &head[..12])) && held.matches(';').count() == 1, "the warm job's lock counts as no job, and the line names the one job the cap counts: {line}");
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "every job left the queue");
}

struct Queued {
    label: &'static str,
    name: &'static str,
    carried: &'static str,
    red_fix: &'static str,
    hold: bool,
    class: super::SlotClass,
}

/// Starts each job against one host slot once the one before it is in line, or for a background job once it has started in its own slot, so they queue in the order given; a job with `hold` stays in the slot until every job queued behind it is in line (a background hold until all others have ended), and the order they started and ended in is returned.
fn order_through_one_slot(jobs: &[Queued]) -> Vec<String> {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let children: Vec<std::process::Child> = jobs
        .iter()
        .enumerate()
        .map(|(index, job)| {
            let name = job.class.slot_name(job.name);
            let script = super::remote_script(&name, job.class);
            let gate = if job.hold { format!("while [ ! -e {}/release-{} ]; do sleep 0.02; done", home.path().display(), job.label) } else { ":".to_owned() };
            let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", label = job.label, log = log.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", &name, &command, "refs/land-offload/job", job.carried, "", "", "0", job.red_fix])
                .env("HOME", home.path())
                .env("PATH", &path)
                .env("NORTH_OFFLOAD_MAX_JOBS", "1")
                .env_remove("NORTH_OFFLOAD_ROOT")
                .stdin(fs::File::open(&bundle).unwrap())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            wait_until_in_line(home.path(), &log, index + 1);
            if matches!(job.class, super::SlotClass::Background) {
                let started = format!("start-{}", job.label);
                assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.lines().any(|line| line == started))), "the background job {} never started in its own slot", job.label);
            }
            child
        })
        .collect();
    let (background, foreground): (Vec<_>, Vec<_>) = children.into_iter().zip(jobs).partition(|(_, job)| matches!(job.class, super::SlotClass::Background));
    let release = |held: &[(std::process::Child, &Queued)]| held.iter().filter(|(_, job)| job.hold).for_each(|(_, job)| fs::write(home.path().join(format!("release-{}", job.label)), "").unwrap());
    release(&foreground);
    for (mut child, _) in foreground {
        assert!(child.wait().unwrap().success());
    }
    release(&background);
    for (mut child, _) in background {
        assert!(child.wait().unwrap().success());
    }
    fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect()
}

#[test]
fn a_red_fix_then_any_land_check_takes_the_first_freed_slot_ahead_of_earlier_worker_builds_while_a_background_sweep_holds_none_of_their_slots() {
    let landing = "SAFE_PUSH_LANDING='1'";
    let order = order_through_one_slot(&[
        Queued { label: "sweep", name: "code-p-main", carried: "", red_fix: "", hold: true, class: super::SlotClass::Background },
        Queued { label: "holder", name: "code-p-main", carried: "", red_fix: "", hold: true, class: super::SlotClass::LandCheck },
        Queued { label: "worker", name: "code-p-main", carried: "", red_fix: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "land", name: "code-p-main", carried: landing, red_fix: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "red-fix", name: "code-p-main", carried: landing, red_fix: "1", hold: false, class: super::SlotClass::LandCheck },
    ]);
    let foreground: Vec<&str> = order.iter().map(String::as_str).filter(|event| *event != "start-sweep").collect();
    assert_eq!(foreground, ["start-holder", "end-holder", "start-red-fix", "end-red-fix", "start-land", "end-land", "start-worker", "end-worker", "end-sweep"], "each queued after the worker build and still took the slot first, the red fix ahead of the land check, and the sweep outlasted them all");
    let at = |event: &str| order.iter().position(|seen| seen == event);
    assert!(at("start-sweep") < at("end-holder"), "the sweep ran in its own slot beside the holder, outside the one-job cap: {order:?}");
}

#[test]
fn repositories_with_jobs_queued_for_one_slot_take_it_in_turn_instead_of_one_draining_first() {
    let job = |label, name, hold| Queued { label, name, carried: "", red_fix: "", hold, class: super::SlotClass::LandCheck };
    let order = order_through_one_slot(&[job("p-holder", "code-p-main", true), job("p-2", "code-p-main", false), job("p-3", "code-p-main", false), job("q-1", "code-q-main", false), job("q-2", "code-q-main", false)]);
    assert_eq!(order, ["start-p-holder", "end-p-holder", "start-q-1", "end-q-1", "start-p-2", "end-p-2", "start-q-2", "end-q-2", "start-p-3", "end-p-3"], "q queued behind both of p's jobs and still ran before p's second");
}

#[test]
fn a_dead_job_leaves_no_ticket_that_blocks_the_queue() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let queue = home.path().join(".cache/land-offload/queue");
    fs::create_dir_all(&queue).unwrap();
    fs::write(queue.join("1-1000000000000000000-1"), "").unwrap();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let output = Command::new("sh")
        .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", "", "0"])
        .env("HOME", home.path())
        .env("PATH", &path)
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
    crate::foundation::shim::write_script(&bin.path().join("nproc"), "echo ${NPROC:-4}\n");
    crate::foundation::shim::write_script(&bin.path().join("ssh"), &format!("echo \"$*\" >>{}\nexit 255\n", home.path().join("ssh-calls").display()));
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let seen = home.path().join("seen");
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |command: String, lander: &str| {
        super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "SAFE_PUSH_LAND_CPUS='6'", "", "", "0", "", "", "", lander], &[])
            .env("HOME", home.path())
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", "1")
            .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let holding = home.path().join("holder-started");
    let holder = job(format!("mkdir -p target && echo built >target/marker && touch {} && sleep 2", holding.display()), "");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(30), || holding.exists()), "the holder never took its slot");
    let queued = job(format!("cat target/marker >>{} && printf '%s %s %s %s %s' \"${}\" \"$SCCACHE_BASEDIRS\" \"$SCCACHE_SERVER_PORT\" \"$CARGO_BUILD_JOBS $RUST_TEST_THREADS $NEXTEST_TEST_THREADS\" \"$SAFE_PUSH_LAND_CPUS\" >{}", home.path().join("marker").display(), north_base::foundation::offload::SLOT_ENV, seen.display()), "");
    let lander_seen = home.path().join("lander-seen");
    let lander = job(format!("printf '%s' \"${}\" >{}", north_base::foundation::offload::SLOT_ENV, lander_seen.display()), "1").wait_with_output().unwrap();
    assert!(lander.status.success(), "{}", String::from_utf8_lossy(&lander.stderr));
    assert!(!String::from_utf8_lossy(&lander.stderr).contains(super::WAITED), "a lander job takes its reserved slot without waiting behind the job cap");
    assert_eq!(fs::read_to_string(&lander_seen).unwrap(), home.path().join(".cache/land-offload/code-p-main-5").display().to_string());
    let queued = queued.wait_with_output().unwrap();
    assert!(holder.wait_with_output().unwrap().status.success());
    assert!(queued.status.success(), "{}", String::from_utf8_lossy(&queued.stderr));
    let stderr = String::from_utf8_lossy(&queued.stderr);
    assert!(stderr.lines().any(|line| line.starts_with(super::WAITED) && line.ends_with("s in line for code-p-main")), "{stderr}");
    let slot = home.path().join(".cache/land-offload/code-p-main");
    assert_eq!(fs::read_to_string(&seen).unwrap(), format!("{0} {0} {1} 4 4 4 4", slot.display(), super::script::slot_ports("code-p-main")[0].1), "a slot job builds and tests on the host's cores divided by its job cap");
    assert_eq!(fs::read_to_string(home.path().join("marker")).unwrap(), "built\n", "a later job in the slot builds on the target the job before it left");
    assert!(!home.path().join("ssh-calls").exists(), "a job for this host runs the slot script here and never reaches ssh");
    let sized = home.path().join("sized");
    let meminfo = home.path().join("meminfo");
    fs::write(&meminfo, "MemTotal:       67108864 kB\n").unwrap();
    let command = format!("printf '%s %s' \"$CARGO_BUILD_JOBS\" \"$SAFE_PUSH_LAND_CPUS\" >{}", sized.display());
    let thirty = Command::new("setsid")
        .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "4"])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("NPROC", "30")
        .env("AGENT_CAPACITY_MEMINFO", &meminfo)
        .env_remove("NORTH_OFFLOAD_MAX_JOBS")
        .env_remove("NORTH_OFFLOAD_ROOT")
        .env_remove("SAFE_PUSH_LAND_CPUS")
        .stdin(fs::File::open(&bundle).unwrap())
        .output()
        .unwrap();
    assert!(thirty.status.success(), "{}", String::from_utf8_lossy(&thirty.stderr));
    assert_eq!(fs::read_to_string(&sized).unwrap(), "5 5", "30 cores and 64 GiB run their six slots at once with 5 cores each, every job under a lease that keeps the host admission headroom, not six 4-core jobs under a fifth held back as well");
}

#[test]
fn a_lander_job_takes_its_reserved_slot_with_no_waited_line_while_another_job_tests_whether_that_slot_is_held_and_the_clock_turns_a_second_on_each_reading() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let root = home.path().join(".cache/land-offload");
    fs::create_dir_all(&root).unwrap();
    for left in ["code-p-main-5.lock", "code-p-main-5.busy"] {
        fs::write(root.join(left), "").unwrap();
    }
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let real = |program: &str| std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join(program)).find(|candidate| candidate.is_file()).unwrap();
    let flock = real("flock");
    let clock = tempfile::tempdir().unwrap();
    let calls = home.path().join("lander-flock-calls");
    crate::foundation::shim::write_script(&clock.path().join("flock"), &format!("echo \"$*\" >>{}\nexec {} \"$@\"\n", shell(&calls), shell(&flock)));
    let readings = home.path().join("readings");
    crate::foundation::shim::write_script(&clock.path().join("date"), &format!("case \"$1\" in +%s | +%s%N) ;; *) exec {} \"$@\" ;; esac\nn=$(cat {1} 2>/dev/null || echo 1000)\necho $((n + 1)) >{1}\n[ \"$1\" = +%s ] && echo $n || echo ${{n}}000000000\n", shell(&real("date")), shell(&readings)));
    let (probing, release) = (home.path().join("probing"), home.path().join("release"));
    let slow = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&slow.path().join("flock"), &format!("[ \"$1 $2\" = '-n 7' ] || exec {0} \"$@\"\n{0} -n 7 || exit 1\ntouch {1}\nwhile [ ! -e {2} ]; do sleep 0.05; done\n", shell(&flock), shell(&probing), shell(&release)));
    let mut probe = Command::new("sh").args(["-c", &format!("root={}\n{}slot_jobs code-p-main", shell(&root), super::jobs_part())]).env("PATH", format!("{}:{path}", slow.path().display())).stdout(std::process::Stdio::null()).spawn().unwrap();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || probing.exists()), "the probe never tested the lander's slot");
    let seen = home.path().join("lander-seen");
    let command = format!("printf '%s' \"${}\" >{}", north_base::foundation::offload::SLOT_ENV, seen.display());
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let lander = super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "0", "", "", "", "1"], &[])
        .env("HOME", home.path())
        .env("PATH", format!("{}:{path}", clock.path().display()))
        .env("NORTH_OFFLOAD_MAX_JOBS", "1")
        .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
        .env_remove("NORTH_OFFLOAD_ROOT")
        .env_remove(north_base::foundation::offload::SLOT_ENV)
        .stdin(fs::File::open(&bundle).unwrap())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let decided = || fs::read_to_string(&calls).is_ok_and(|calls| calls.lines().any(|call| call == "-s 4") || calls.lines().filter(|call| *call == "-n 9").count() > 1);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), decided), "the lander neither took its slot nor tried it a second time: {:?}", fs::read_to_string(&calls));
    fs::write(&release, "").unwrap();
    let lander = lander.wait_with_output().unwrap();
    assert!(probe.wait().unwrap().success());
    let stderr = String::from_utf8_lossy(&lander.stderr);
    assert!(lander.status.success(), "{stderr}");
    assert!(!stderr.contains(super::WAITED), "neither a probe of the lander's reserved slot nor a second turning on the clock while it takes the slot reports the lander as waiting in line (north#815): {stderr}");
    assert_eq!(fs::read_to_string(&seen).unwrap(), root.join("code-p-main-5").display().to_string());
}

fn remote_reports(command: &str, marker: &str) -> (Option<u8>, Option<u8>, String) {
    let slot = tempfile::tempdir().unwrap();
    let script = format!("set -eu\nslot={} command={} limit=10 ran={}\n{}", shell(slot.path()), super::quote(command), super::quote(marker), run_part());
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let output = Command::new("sh").args(["-c", &script]).env("HOME", slot.path()).env("PATH", path).output().unwrap();
    let mut shown = Vec::new();
    let ran = super::relay(output.stdout.as_slice(), &mut shown, marker, || {});
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
    let never_ran = super::relay(b"bash: --: invalid option\nUsage: bash [option] ...\n".as_slice(), &mut shown, marker, || {});
    assert!(super::verdict(Some(0), never_ran).is_err(), "a remote that exits 0 without reporting its command passed");
    assert_eq!(String::from_utf8(shown).unwrap(), "bash: --: invalid option\nUsage: bash [option] ...\n");
}

#[test]
fn a_connection_that_drops_before_the_remote_reports_its_command_reaches_no_verdict() {
    for exited in [Some(255), Some(1), None] {
        assert!(super::verdict(exited, None).is_err(), "ssh {exited:?} with no reported exit read as the command's verdict");
    }
    assert_eq!(super::verdict(Some(255), Some(1)), Ok(1), "a reported failure stays the command's verdict when the connection drops after it");
    assert_eq!(super::verdict(Some(255), Some(0)), Ok(0), "a reported pass stays a pass when the connection drops after it");
    for refusal in [super::SLOTS_BUSY, super::WRONG_TREE, super::BESIDE_MISSING] {
        assert_eq!(super::verdict(Some(refusal), None), Ok(refusal), "the remote script's own refusal keeps its code");
    }
    assert_eq!(super::NO_VERDICT, 75, "safe-push reads exit 75 as no verdict, never the lane's failure");
}

#[test]
fn every_slot_of_a_checkout_serves_sccache_on_its_own_stable_port_in_range() {
    let ports = super::script::slot_ports("code-north-main");
    assert_eq!(ports, super::script::slot_ports("code-north-main"));
    assert_eq!(ports.iter().map(|(slot, _)| slot.as_str()).collect::<Vec<_>>(), ["code-north-main", "code-north-main-1", "code-north-main-2", "code-north-main-3", "code-north-main-4", "code-north-main-5"]);
    let mut numbers: Vec<u32> = ports.iter().map(|(_, port)| *port).collect();
    assert!(numbers.iter().all(|port| (20000..40000).contains(port)), "{numbers:?}");
    numbers.dedup();
    assert_eq!(numbers.len(), super::SLOTS as usize);
}

#[test]
fn a_slot_job_that_offloads_back_to_its_own_host_runs_the_nested_job_without_waiting_out_the_queue() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let repo = home.path().join("code/p/main");
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let other = git(&repo, &["rev-parse", "HEAD"]);
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let inner = home.path().join("inner.sh");
    fs::write(&inner, &script).unwrap();
    let nested = |head: &str, ran: &Path| format!("setsid -w sh -c \"$(cat {inner})\" offload code/p/main '' {head} 30 code-p-main 'printf %s \"${slot_env}\" >{ran}' refs/land-offload/job '' '' '' 0 <{bundle}", inner = inner.display(), ran = ran.display(), bundle = bundle.display(), slot_env = north_base::foundation::offload::SLOT_ENV);
    let ran = home.path().join("inner-ran");
    let ran_elsewhere = home.path().join("inner-ran-elsewhere");
    let command = format!("{} && {}", nested(&head, &ran), nested(&other, &ran_elsewhere));
    let mut outer = Command::new("setsid")
        .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "0"])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("NORTH_OFFLOAD_MAX_JOBS", "1")
        .env_remove(north_base::foundation::offload::SLOT_ENV)
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(fs::File::open(&bundle).unwrap())
        .stdout(fs::File::create(home.path().join("outer.out")).unwrap())
        .stderr(fs::File::create(home.path().join("outer.err")).unwrap())
        .spawn()
        .unwrap();
    north_base::platform::wait::until(std::time::Duration::from_secs(10), || outer.try_wait().unwrap().is_some());
    let finished = outer.try_wait().unwrap();
    if finished.is_none() {
        let _ = rustix::process::kill_process_group(rustix::process::Pid::from_raw(outer.id() as i32).unwrap(), rustix::process::Signal::Kill);
        let _ = outer.wait();
    }
    let err = fs::read_to_string(home.path().join("outer.err")).unwrap_or_default();
    assert!(finished.is_some_and(|status| status.success()), "the nested jobs did not run within 10 s while their caller held the host's one-job cap: {err}");
    assert_eq!(fs::read_to_string(&ran).unwrap(), home.path().join(".cache/land-offload/code-p-main").display().to_string(), "the nested job ran in the slot its caller holds");
    assert_eq!(fs::read_to_string(&ran_elsewhere).unwrap(), home.path().join(".cache/land-offload/code-p-main-1").display().to_string(), "the nested job at another head took a free slot past its caller's one-job cap");
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "the nested jobs left no ticket in the queue");
}

// KEEP(bug): until 2026-11-07 every offloaded land check from the laptop opened a second ssh connection, about 1.8 s over its 230 ms link to gex45, only to ask whether the lander runtime was there; the job now asks in its own connection and stages only on this exit (north#697)
#[test]
fn a_job_for_a_host_without_the_lander_runtime_says_so_before_taking_a_slot_and_with_it_runs() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let runtime = "0123456789abcdef0123456789abcdef01234567";
    let job = || {
        Command::new("sh")
            .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", runtime, "0"])
            .env("HOME", home.path())
            .env("PATH", &path)
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .output()
            .unwrap()
    };
    let missing = job();
    assert_eq!(missing.status.code(), Some(i32::from(super::RUNTIME_MISSING)), "{}", String::from_utf8_lossy(&missing.stderr));
    assert!(!home.path().join(".cache/land-offload/code-p-main").exists(), "the job took no slot");
    let shipped = home.path().join(".local/state/north/runtimes").join(runtime);
    let marker = shipped.join(crate::runtime::layout::tree_marker());
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(marker, "").unwrap();
    crate::foundation::shim::write_script(&shipped.join("north"), NORTH_SHIM);
    let ran = job();
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
}

// KEEP(bug): on 2026-11-08 lane checks ranked level with the lander's probe jobs took free code-north-main slots on gex45, and probe jobs queued behind them
#[test]
fn a_lane_check_waits_while_lane_checks_hold_their_share_of_a_hosts_slots_and_a_lander_job_queued_after_it_takes_a_free_slot_first() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let release = home.path().join("release");
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let spawn = |label: &str, lander: &str, hold: bool| {
        let gate = if hold { format!("while [ ! -e {} ]; do sleep 0.02; done", release.display()) } else { ":".to_owned() };
        let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", log = log.display());
        Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "SAFE_PUSH_LANDING='1'", "", "", "0", "", "", "", lander])
            .env("HOME", home.path())
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", (super::OTHER_JOBS + 2).to_string())
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let lanes: Vec<std::process::Child> = (0..super::OTHER_JOBS).map(|index| {
        let child = spawn(&format!("lane-{index}"), "", true);
        wait_until_in_line(home.path(), &log, index as usize + 1);
        child
    }).collect();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.matches("start-lane-").count() == super::OTHER_JOBS as usize)), "the lane checks never took their slots");
    let queued = spawn("queued-lane", "", false);
    wait_until_in_line(home.path(), &log, super::OTHER_JOBS as usize + 1);
    let mut holding = spawn("lander", "1", true);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.contains("start-lander"))), "the lander job never took its reserved slot");
    let second = spawn("second-lander", "1", false).wait().unwrap();
    assert!(second.success(), "a lander job queued after the lane check takes the slot under the job cap while the lander's reserved slot is held");
    assert!(!fs::read_to_string(&log).unwrap().contains("start-queued-lane"), "the lane check stays queued while lane checks hold their share, though the job cap has room");
    fs::write(&release, "").unwrap();
    for mut lane in lanes {
        assert!(lane.wait().unwrap().success());
    }
    assert!(holding.wait().unwrap().success());
    assert!(queued.wait_with_output().unwrap().status.success());
    let order: Vec<String> = fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
    assert_eq!(order.iter().filter(|event| event.contains("lander")).count(), 4, "both lander jobs ran while the lane checks held their slots: {order:?}");
    assert_eq!(order.last().map(String::as_str), Some("end-queued-lane"), "{order:?}");
}

#[test]
fn a_run_whose_connection_is_killed_in_the_middle_still_reports_its_real_exit_and_output_to_a_fresh_connection() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let marker = "north-offload-ran:job-lost";
    let script = super::remote_script("code-p-main", super::SlotClass::LandCheck);
    let command = "echo before; sleep 2; echo after; exit 3";
    let mut remote = Command::new("setsid")
        .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", "", "0", "", marker])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env_remove(north_base::foundation::offload::SLOT_ENV)
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(fs::File::open(&bundle).unwrap())
        .stdout(std::process::Stdio::piped())
        .stderr(fs::File::create(home.path().join("remote.err")).unwrap())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(remote.stdout.take().unwrap());
    let mut line = String::new();
    while !line.starts_with(super::RUNNING) {
        line.clear();
        assert!(std::io::BufRead::read_line(&mut lines, &mut line).unwrap() > 0, "the remote ended before it ran: {}", fs::read_to_string(home.path().join("remote.err")).unwrap_or_default());
    }
    drop(lines);
    let exited = home.path().join(".cache/land-offload/results/job-lost.exit");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || exited.exists()), "the run stopped with its connection instead of finishing: {}", fs::read_to_string(home.path().join("remote.err")).unwrap_or_default());
    assert_eq!(remote.wait().unwrap().code(), Some(3), "the remote script outlived its connection and ended with the command's exit");
    let read = Command::new("sh").args(["-c", &super::script::reattach_script(), "offload-reattach", marker, "30"]).env("HOME", home.path()).output().unwrap();
    let mut shown = Vec::new();
    let ran = super::relay(read.stdout.as_slice(), &mut shown, marker, || {});
    assert_eq!(super::verdict(read.status.code().and_then(|code| u8::try_from(code).ok()), ran), Ok(3), "a fresh connection reads the real exit: {}", String::from_utf8_lossy(&read.stderr));
    assert!(String::from_utf8(shown).unwrap().contains("after"), "a fresh connection reads the output written after the loss");
}

#[test]
fn a_connection_that_ends_after_the_run_started_is_asked_again_until_the_host_answers_and_never_when_it_cannot_help() {
    let lost = (Some(255), None);
    let mut asked = 0;
    let answered = super::reattach::ride_out(lost, true, std::time::Duration::from_secs(5), std::time::Duration::ZERO, || {
        asked += 1;
        if asked < 3 { lost } else { (Some(0), Some(3)) }
    });
    assert_eq!((answered, asked), ((Some(0), Some(3)), 3));
    for (first, started, window) in [(lost, false, 5), ((Some(255), Some(1)), true, 5), ((Some(1), None), true, 5), (lost, true, 0)] {
        let mut asked = 0;
        let kept = super::reattach::ride_out(first, started, std::time::Duration::from_secs(window), std::time::Duration::ZERO, || {
            asked += 1;
            (Some(0), Some(0))
        });
        assert_eq!((kept, asked), (first, 0), "{first:?} started {started} window {window}");
    }
}

#[test]
fn the_offload_master_starts_as_the_main_process_of_a_unit_of_its_own_and_a_client_never_forks_one() {
    let arguments = super::master::unit_arguments("tom@cpx62.example", "/run/user/1/north-offload-%C", Some("/run/agent"));
    let at = |word: &str| arguments.iter().position(|argument| argument == word).unwrap_or_else(|| panic!("{word} is not in {arguments:?}"));
    assert!(at("--user") < at("--") && at("--collect") < at("--"));
    assert_eq!(arguments[at("--unit") + 1], "north-offload-master-tom-cpx62-example");
    assert!(arguments.contains(&"--setenv=SSH_AUTH_SOCK=/run/agent".to_owned()));
    assert!(at("ssh") == at("--") + 1 && at("-M") > at("ssh") && at("-N") > at("ssh") && !arguments.contains(&"-f".to_owned()), "the master stays in the foreground as the unit's process: {arguments:?}");
    assert_eq!(arguments.last().map(String::as_str), Some("tom@cpx62.example"));
    let client = super::master::options("");
    assert!(client.contains(&"ControlMaster=no".to_owned()) && !client.iter().any(|option| option.starts_with("ControlPersist") || option == "ControlMaster=auto"), "{client:?}");
}
