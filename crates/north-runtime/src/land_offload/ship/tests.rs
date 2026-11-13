use super::script::{VERIFY, run_part};
use super::{CommitId, Land, WRONG_TREE, bundle, carried, job_ref};
use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn carried_from(caller: &[(&str, &str)]) -> String {
    let land = Land { ssh: "", checkout: Path::new("."), base: Some(CommitId("b".to_owned())), head: CommitId("h".to_owned()), timeout_seconds: 10, command: "", class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
    carried(&land, |name| caller.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned()))
}

const FLAKE_WITHOUT_DEV_SHELL: &str = "{ outputs = { checks = 1; }; }";

const NIX_SHIM: &str = "echo \"nix $*\"\n";

const NORTH_SHIM: &str = r#"case "$1 $2 $3" in
  "capacity host admit-job") echo "$4" >>"$HOME/admit-calls"; [ ! -e "$HOME/refuse" ] || { echo "refused for the test"; exit 75; }; [ "${4:-}" != --landing ] || [ ! -e "$HOME/landing-full" ] || { echo "land checks charge the cores for the test"; exit 75; }; exit 0 ;;
  "capacity offload closed") [ ! -e "$HOME/closed" ] || { echo "gex45"; exit 75; }; exit 0 ;;
  "capacity offload line") case "$4" in wait) shift 4; echo "$*" >>"$HOME/line-asks"; [ ! -e "$HOME/line-full" ] || { echo "expected wait 700 s, over the rank's deadline 600 s"; exit 96; }; echo "expected wait 0 s"; exit 0 ;; record) shift 4; echo "$*" >>"$HOME/line-calls"; exit 0 ;; esac ;;
esac
case "$1 $2" in
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
    remote_run(command, flake, caller, home).0
}

fn remote_run(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>) -> (String, String) {
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
        .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h carried={}\nproved() {{ :; }}\n{}", shell(slot.path()), shell(Path::new(command)), super::quote(&carried_from(caller)), run_part())])
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
        .env("HOME", home.unwrap_or(fresh_home.path())).env("TMPDIR", home.unwrap_or(fresh_home.path()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let shown = slot.path().display().to_string();
    (String::from_utf8(output.stdout).unwrap().replace(&shown, "SLOT"), String::from_utf8(output.stderr).unwrap().replace(&shown, "SLOT"))
}

pub(super) fn shell(word: &Path) -> String {
    super::quote(&word.display().to_string())
}

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
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
    let (_, stderr) = remote_run("true", None, &[("SAFE_PUSH_LANDING", "1")], None);
    assert!(stderr.lines().any(|line| line == "lease run --class heavy --owner land-check --timeout-seconds 10"), "{stderr}");
    let (_, stderr) = remote_run("true", None, &[], None);
    assert!(stderr.lines().any(|line| line == "lease run --class heavy --owner land-offload --timeout-seconds 10"), "{stderr}");
}

#[test]
fn operator_check_line_runs_only_on_the_shipped_commit_and_pins_south_beside_it_as_check_owners_does() {
    let (dir, first, second) = repository_with_two_commits();
    let refused = verify(dir.path(), &first);
    assert_eq!(refused.status.code(), Some(i32::from(WRONG_TREE)));
    assert!(refused.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&refused.stderr), format!("offload: refusing to run: {} is on {second}, not the shipped {first}\n", dir.path().display()));
    let ran = verify(dir.path(), &second);
    assert!(ran.status.success());
    let mut slot_waits_ended = 0;
    super::relay(ran.stdout.as_slice(), &mut Vec::new(), "north-offload-ran:job", "", || slot_waits_ended += 1);
    assert_eq!(slot_waits_ended, 1, "the line the remote prints once it holds its slot ends the slot wait a deadline above the offload excuses");
    assert_eq!(String::from_utf8(ran.stdout).unwrap().lines().next(), Some(format!("offload: running {second} in {}", dir.path().display()).as_str()));
    fs::write(dir.path().join("tracked.rs"), "fn shipped() {}\n").unwrap();
    git(dir.path(), &["add", "tracked.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "third"]);
    let third = git(dir.path(), &["rev-parse", "HEAD"]);
    let plant = || {
        fs::write(dir.path().join("tracked.rs"), "fn another_lanes() {}\n").unwrap();
        fs::write(dir.path().join("foreign.rs"), "mod planted;\n").unwrap();
    };
    plant();
    let nested = Command::new("sh").args(["-c", &format!("set -eu\nslot={} head={} nested=1\n{}", shell(dir.path()), super::quote(&third), VERIFY.replace("@WRONG_TREE@", &WRONG_TREE.to_string()))]).output().unwrap();
    assert_eq!(nested.status.code(), Some(i32::from(WRONG_TREE)), "a nested job in a slot holding a changed tracked file and a foreign file refuses, since the slot is its caller's to reset");
    assert!(nested.stdout.is_empty(), "the refusal comes before the run starts");
    let why = String::from_utf8_lossy(&nested.stderr);
    assert!(why.contains(&format!("does not hold exactly the tree of {third}")) && why.contains("M tracked.rs") && why.contains("?? foreign.rs"), "{why}");
    let repaired = verify(dir.path(), &third);
    assert!(repaired.status.success(), "a job that owns the slot resets it to the commit and proves it again: {}", String::from_utf8_lossy(&repaired.stderr));
    assert_eq!(fs::read_to_string(dir.path().join("tracked.rs")).unwrap(), "fn shipped() {}\n", "the reset restores the commit's tracked file");
    assert!(!dir.path().join("foreign.rs").exists(), "the reset removes the foreign file");
    git(dir.path(), &["checkout", "-q", "--detach", &second]);
    let command = "check-owners --north-root $HOME/code/north/main --south-root $HOME/code/south/main --nixos-root ~/code/nixos-config/main/agents $HOME/code/gone/main $HOME/code/south/mainline";
    let head_of = |path: &Path| path.ends_with("code/south/main").then(|| CommitId(first.clone())).or_else(|| path.ends_with("code/nixos-config/main").then(|| CommitId("b".repeat(40))));
    let pinned = super::beside::pin(command, "code/north/main", Path::new("/h"), head_of);
    assert_eq!(pinned.command, "check-owners --north-root $HOME/code/north/main --south-root $NORTH_OFFLOAD_BESIDE/south --nixos-root $NORTH_OFFLOAD_BESIDE/nixos-config/agents $HOME/code/gone/main $HOME/code/south/mainline");
    assert_eq!(pinned.pins, format!("south={first} nixos-config={}", "b".repeat(40)));
    let operator = "CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/operator/ config operator check $HOME/code/south/main";
    let pinned = super::beside::pin(operator, "code/north/main", Path::new("/h"), head_of);
    assert_eq!(pinned.command, "CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/operator/ config operator check $NORTH_OFFLOAD_BESIDE/south");
    assert_eq!(pinned.pins, format!("south={first}"));
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
                let land = Land { ssh: "", checkout: dir.path(), base: Some(base.clone()), head: head.clone(), timeout_seconds: 1, command: "", class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
                let checkout = dir.path();
                scope.spawn(move || {
                    let file = bundle(&land, reference, None).unwrap();
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
    let script = super::script::slot_part().replace("@JOBS@", &super::script::jobs_part()).replace("@ADMIT@", super::script::ADMIT).replace("@SETTLE@", "0").replace("@ROOT@", super::OFFLOAD_ROOT).replace("@LINE@", "lock_wait=0\nline_note() { :; }\n").replace("@SLOTS_BUSY@", "96").replace("@SLOTS@", "1").replace("@SLOT_PORTS@", "");
    let script = format!("{script}cp \"$placed/$s\" \"$HOME/read-by-the-job\" 2>/dev/null || :\n");
    let ship = |north: Option<&Path>| {
        let mut shell = Command::new("sh");
        shell.args(["-c", &script, "offload", "code/p/main", "", &head, "10", "code-p-main", "true", "refs/land-offload/job"]).env("HOME", home.path()).env("NORTH_OFFLOAD_SETTLE_SECONDS", "0").env_remove("NORTH_OFFLOAD_ROOT").stdin(fs::File::open(&bundle).unwrap());
        north.map(|north| shell.env("AGENT_CAPACITY_NORTH", north));
        shell.output().unwrap()
    };
    let records = home.path().join(north_base::foundation::offload::SLOT_RECORDS);
    let record = records.join("code-p-main");
    let read = home.path().join("read-by-the-job");
    let output = ship(None);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(&read).unwrap_or_default(), "code/p/main\n", "the script recorded no checkout for the slot while it held it, so the sandbox broker cannot tell which project a request from its job belongs to");
    assert!(!record.exists(), "the record outlived the job that held the slot, so a directory made under a released slot's name is served as that project");
    assert_eq!(git(&home.path().join(".cache/land-offload/code-p-main"), &["rev-parse", "HEAD"]), head);
    assert!(half_made.is_dir(), "a fetch's auto maintenance pruned a worktree entry that has no gitdir yet, as it would one git worktree add is still creating");
    fs::remove_dir_all(&records).unwrap();
    fs::write(&records, "").unwrap();
    let north = home.path().join("north");
    crate::foundation::shim::write_script(&north, "printf '%s|%s\\n' \"$PWD\" \"$*\" >>\"$HOME/asked\"\n");
    let output = ship(Some(&north));
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{told}");
    let asked = fs::read_to_string(home.path().join("asked")).unwrap_or_default();
    assert!(asked.lines().any(|line| line == format!("{}|capacity offload slot record code-p-main", repo.display())), "a script that could not write its slot's record, as one run inside a session's domain cannot, asked north for no record from the main checkout, so the broker refuses every request its job makes: north was asked {asked:?}; {told}");
    assert!(!told.contains("cannot record"), "{told}");
    let older = home.path().join("older");
    crate::foundation::shim::write_script(&older, "echo 'usage: north capacity offload' >&2\nexit 2\n");
    let current = home.path().join(".local/state/north/current/north");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(&current, "printf '%s|%s\\n' \"$PWD\" \"$*\" >>\"$HOME/asked-current\"\n");
    let output = ship(Some(&older));
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{told}");
    let asked = fs::read_to_string(home.path().join("asked-current")).unwrap_or_default();
    assert!(asked.lines().any(|line| line == format!("{}|capacity offload slot record code-p-main", repo.display())), "a north that predates the record verb answered with its usage and the script asked no other north for the record: {asked:?}; {told}");
    assert!(!told.contains("cannot record"), "{told}");
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
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let jobs: Vec<std::process::Child> = ["a", "b", "c", "d"]
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let release = home.path().join("release");
            let command = format!("echo start-{label} >>{log}; while [ ! -e {release} ]; do sleep 0.02; done; echo end-{label} >>{log}", log = log.display(), release = release.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", ""])
                .env("HOME", home.path())
                .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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
    assert!(line.contains("code-p-main runs 1 of its 6 slots, the host 1 jobs; the host runs at most 1 jobs;") && held.starts_with(&format!("code-p-main.lock {} since ", &head[..12])) && held.matches(';').count() == 1, "the warm job's lock counts as no job, and the line names the one job the cap counts: {line}");
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "every job left the queue");
}

pub(crate) struct Queued {
    pub(crate) label: &'static str,
    pub(crate) name: &'static str,
    pub(crate) carried: &'static str,
    pub(crate) red_fix: &'static str,
    pub(crate) owner: &'static str,
    pub(crate) urgent: &'static str,
    pub(crate) hold: bool,
    pub(crate) class: super::SlotClass,
}

/// Starts each job against one host slot once the one before it is in line, or for one with `hold` once it has started in its slot, so they queue in the order given; a job with `hold` stays in the slot until every job queued behind it is in line (a background hold until all others have ended), and the order they started and ended in is returned.
pub(crate) fn order_through_one_slot(jobs: &[Queued]) -> Vec<String> {
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
            let script = remote_script(&name, job.class);
            let gate = if job.hold { format!("while [ ! -e {}/release-{} ]; do sleep 0.02; done", home.path().display(), job.label) } else { ":".to_owned() };
            let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", label = job.label, log = log.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", &name, &command, "refs/land-offload/job", job.carried, job.owner, "", job.red_fix, "", "", "", "", "", job.urgent])
                .env("HOME", home.path())
                .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
                .env("PATH", &path)
                .env("NORTH_OFFLOAD_MAX_JOBS", "1")
                .env_remove("NORTH_OFFLOAD_ROOT")
                .stdin(fs::File::open(&bundle).unwrap())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            wait_until_in_line(home.path(), &log, index + 1);
            if job.hold {
                let started = format!("start-{}", job.label);
                assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.lines().any(|line| line == started))), "the job {} never started in its slot", job.label);
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
        Queued { label: "sweep", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: true, class: super::SlotClass::Background },
        Queued { label: "holder", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: true, class: super::SlotClass::LandCheck },
        Queued { label: "worker", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "land", name: "code-p-main", carried: landing, red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "red-fix", name: "code-p-main", carried: landing, red_fix: "1", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "forged", name: "code-p-main", carried: "SAFE_PUSH_LANDING='after'", red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "after", name: "code-p-main", carried: "SAFE_PUSH_LANDING='after'", red_fix: "", owner: "land-check", urgent: "", hold: false, class: super::SlotClass::LandCheck },
    ]);
    let foreground: Vec<&str> = order.iter().map(String::as_str).filter(|event| *event != "start-sweep").collect();
    assert_eq!(foreground, ["start-holder", "end-holder", "start-red-fix", "end-red-fix", "start-after", "end-after", "start-land", "end-land", "start-worker", "end-worker", "start-forged", "end-forged", "end-sweep"], "each queued after the worker build and still took the slot first, the red fix ahead of the lander's after-landing job and that ahead of the hand-in land check, an after word with no lander owner stayed behind the worker, and the sweep outlasted them all");
    let at = |event: &str| order.iter().position(|seen| seen == event);
    assert!(at("start-sweep") < at("end-holder"), "the sweep ran in its own slot beside the holder, outside the one-job cap: {order:?}");
}

#[test]
fn repositories_with_jobs_queued_for_one_slot_take_it_in_turn_instead_of_one_draining_first() {
    let job = |label, name, hold| Queued { label, name, carried: "", red_fix: "", owner: "", urgent: "", hold, class: super::SlotClass::LandCheck };
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
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let output = Command::new("sh")
        .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", ""])
        .env("HOME", home.path())
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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
    crate::foundation::shim::write_script(&bin.path().join("ssh"), &format!("echo \"$*\" >>{}\nexit 255\n", home.path().join("ssh-calls").display()));
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let seen = home.path().join("seen");
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |command: String, lander: &str| {
        super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "SAFE_PUSH_LAND_CPUS='6'", "", "", "", "", "", lander], &[])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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
    let queued = job(format!("cat target/marker >>{} && printf '%s %s %s' \"${}\" \"$SCCACHE_BASEDIRS\" \"$SCCACHE_SERVER_PORT\" >{}", home.path().join("marker").display(), north_base::foundation::offload::SLOT_ENV, seen.display()), "");
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
    assert_eq!(fs::read_to_string(&seen).unwrap(), format!("{0} {0} {1}", slot.display(), super::script::slot_ports("code-p-main")[0].1));
    assert_eq!(fs::read_to_string(home.path().join("marker")).unwrap(), "built\n", "a later job in the slot builds on the target the job before it left");
    assert!(!home.path().join("ssh-calls").exists(), "a job for this host runs the slot script here and never reaches ssh");
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
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let lander = super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "", "", "", "", "", "", "1"], &[])
        .env("HOME", home.path())
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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

const PROVED: &str = "0123456789abcdef0123456789abcdef01234567";

fn remote_reports(command: &str, marker: &str) -> (Option<u8>, Option<u8>, String) {
    let slot = tempfile::tempdir().unwrap();
    let script = format!("set -eu\nslot={} command={} limit=10 ran={} proof=secret proved_tree={PROVED}\nproved() {{ :; }}\n{}", shell(slot.path()), super::quote(command), super::quote(marker), run_part());
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let output = Command::new("sh").args(["-c", &script]).env("HOME", slot.path()).env("TMPDIR", slot.path()).env("PATH", path).output().unwrap();
    let mut shown = Vec::new();
    let relayed = super::relay(output.stdout.as_slice(), &mut shown, marker, "secret", || {});
    assert_eq!(relayed.tree.as_deref(), Some(PROVED), "the tree the host proved reaches the client on the line its secret opens");
    (output.status.code().and_then(|code| u8::try_from(code).ok()), relayed.ran, String::from_utf8(shown).unwrap())
}

#[test]
fn an_offload_passes_only_when_the_remote_reports_its_command_ran_and_succeeded() {
    let marker = "north-offload-ran:job-1";
    let (exited, ran, shown) = remote_reports("printf partial; true", marker);
    assert_eq!((super::verdict(exited, ran), shown.as_str()), (Ok(0), "partial\n"));
    let (exited, ran, _) = remote_reports("false", marker);
    assert_eq!(super::verdict(exited, ran), Ok(1));
    let mut shown = Vec::new();
    let never_ran = super::relay(b"bash: --: invalid option\nUsage: bash [option] ...\n".as_slice(), &mut shown, marker, "", || {}).ran;
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
fn a_slot_job_that_offloads_back_to_its_own_host_runs_the_nested_job_without_waiting_out_the_queue_and_ends_it_with_its_check_and_a_held_slot_takes_only_jobs_with_its_token_while_its_caller_lives() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let repo = home.path().join("code/p/main");
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let other = git(&repo, &["rev-parse", "HEAD"]);
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let inner = home.path().join("inner.sh");
    fs::write(&inner, &script).unwrap();
    let nested = |head: &str, ran: &Path, then: &str| format!("setsid -w sh -c \"$(cat {inner})\" offload code/p/main '' {head} 30 code-p-main 'printf %s \"${slot_env}\" >{ran}{then}' refs/land-offload/job '' '' '' 0 <{bundle}", inner = inner.display(), ran = ran.display(), bundle = bundle.display(), slot_env = north_base::foundation::offload::SLOT_ENV);
    let job = |command: &str, cap: &str, log: &str| {
        let mut outer = Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", ""])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", cap)
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(fs::File::create(home.path().join(format!("{log}.out"))).unwrap())
            .stderr(fs::File::create(home.path().join(format!("{log}.err"))).unwrap())
            .spawn()
            .unwrap();
        north_base::platform::wait::until(std::time::Duration::from_secs(10), || outer.try_wait().unwrap().is_some());
        let finished = outer.try_wait().unwrap();
        if finished.is_none() {
            let _ = rustix::process::kill_process_group(rustix::process::Pid::from_raw(outer.id() as i32).unwrap(), rustix::process::Signal::Kill);
            let _ = outer.wait();
        }
        (finished.is_some_and(|status| status.success()), fs::read_to_string(home.path().join(format!("{log}.err"))).unwrap_or_default())
    };
    let ran = home.path().join("inner-ran");
    let ran_elsewhere = home.path().join("inner-ran-elsewhere");
    let (passed, err) = job(&format!("{} && {}", nested(&head, &ran, ""), nested(&other, &ran_elsewhere, "")), "1", "outer");
    assert!(passed, "the nested jobs did not run within 10 s while their caller held the host's one-job cap: {err}");
    let slot = |name: &str| home.path().join(".cache/land-offload").join(name).display().to_string();
    assert_eq!(fs::read_to_string(&ran).unwrap(), slot("code-p-main"), "the nested job ran in the slot its caller holds");
    assert_eq!(fs::read_to_string(&ran_elsewhere).unwrap(), slot("code-p-main-1"), "the nested job at another head took a free slot past its caller's one-job cap");
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "the nested jobs left no ticket in the queue");
    let lingering = home.path().join("lingering-ran");
    let ended = home.path().join("lingering-ended");
    let said = home.path().join("lingering.err");
    let linger = format!("setsid -w sh -c \"$(cat {inner})\" offload code/p/main '' {head} 30 code-p-main 'printf %s \"${slot_env}\" >{ran}; sleep 30; : >{ended}' refs/land-offload/job '' '' '' '' north-offload-ran:linger <{bundle} >/dev/null 2>{said} & until [ -e {ran} ]; do sleep 0.05; done", inner = inner.display(), ran = lingering.display(), ended = ended.display(), said = said.display(), bundle = bundle.display(), slot_env = north_base::foundation::offload::SLOT_ENV);
    let (passed, err) = job(&linger, "", "caller");
    assert!(passed && north_base::platform::wait::until(std::time::Duration::from_secs(10), || lingering.exists()), "the caller did not start its nested job: {err} {}", fs::read_to_string(&said).unwrap_or_default());
    let free = || fs::File::open(home.path().join(".cache/land-offload/code-p-main.nested")).is_ok_and(|busy| rustix::fs::flock(&busy, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_ok());
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(10), free), "a nested job still holds its slot 10 s after the check that started it ended");
    assert!(!ended.exists(), "the nested job ran to its own end instead of ending with its check");
    let logged = fs::read_to_string(home.path().join(".cache/land-offload/nested.log")).unwrap_or_default();
    assert!(logged.contains("the check that started the nested job") && logged.contains("has ended, so the job ends with it"), "the watcher records why it ended the nested job: {logged}");
    let taker = home.path().join("taker-ran");
    let (passed, err) = job(&format!("printf %s \"${}\" >{}", north_base::foundation::offload::SLOT_ENV, taker.display()), "", "taker");
    assert!(passed, "the next job did not run: {err}");
    assert_eq!(fs::read_to_string(&taker).unwrap(), slot("code-p-main"), "the next job takes the slot the ended nested job left");
    let slot_env = north_base::foundation::offload::SLOT_ENV;
    let on_host = |limit: &str, header: &[u8], lander_slot: &str| {
        super::on_host("", &script, &["code/p/main", "", &head, limit, "code-p-main", "", "refs/land-offload/job", "", "", "", "", "", "", lander_slot, &header.len().to_string()], &[])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", "1")
            .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(slot_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let bundled = fs::metadata(&bundle).unwrap().len();
    let token = "ab".repeat(32);
    let take = super::hold::header(&super::Hold::Take { host: String::new() }, Some(&token));
    let header = bundle::spilled(super::HOLD_COMMAND, "SAFE_PUSH_LANDING='1'", "", "secret", false, &take, bundled);
    let mut hold = on_host("99999", &header, "1");
    let mut stdin = hold.stdin.take().unwrap();
    stdin.write_all(&header).and_then(|()| std::io::copy(&mut fs::File::open(&bundle).unwrap(), &mut stdin).map(drop)).unwrap();
    let mut said = std::io::BufReader::new(hold.stdout.take().unwrap()).lines();
    let held = said.find_map(|line| line.ok()?.strip_prefix(super::hold::HOLDING).map(|rest| rest.split(' ').next().unwrap_or("").to_owned()));
    assert_eq!(held.as_deref(), Some("code-p-main"), "a hold takes a free slot and never the lander's reserved one, whatever its lander_slot says");
    let join = |token: &str, command: &str| {
        let fields = super::hold::header(&super::Hold::Join { slot: "code-p-main".to_owned(), token: token.to_owned() }, None);
        let header = bundle::spilled(command, "", "", "secret", false, &fields, bundled);
        let mut joined = on_host("30", &header, "");
        let fed = bundle::feed(header, fs::File::open(&bundle).unwrap(), joined.stdin.take().unwrap(), false);
        let output = joined.wait_with_output().unwrap();
        fed.join().unwrap();
        output
    };
    let (joined_ran, nested_ran, nested_code) = (home.path().join("joined-ran"), home.path().join("nested-in-hold-ran"), home.path().join("nested-in-hold-code"));
    let joined = join(&token, &format!("printf %s \"${slot_env}\" >{}; code=0; {} || code=$?; echo $code >{}", joined_ran.display(), nested(&head, &nested_ran, ""), nested_code.display()));
    assert!(joined.status.success(), "a job with the hold's token did not run beside it: {}", String::from_utf8_lossy(&joined.stderr));
    assert_eq!(fs::read_to_string(&joined_ran).unwrap(), slot("code-p-main"), "a job with the hold's token runs in the held slot past the host's one-job cap");
    assert_eq!(fs::read_to_string(&nested_code).unwrap().trim(), WRONG_TREE.to_string(), "a job started inside a held slot without the hold's token is refused");
    assert!(!nested_ran.exists());
    let stranger = join(&"cd".repeat(32), ":");
    assert_eq!(stranger.status.code(), Some(i32::from(WRONG_TREE)), "{}", String::from_utf8_lossy(&stranger.stderr));
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).unwrap().count(), 0, "the joins took no ticket");
    drop(stdin);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(10), || hold.try_wait().unwrap().is_some()), "a hold whose caller closed its input kept its slot");
    assert!(!home.path().join(".cache/land-offload/code-p-main.hold").exists(), "an ended hold left its token's hash behind");
    let calls = fs::read_to_string(home.path().join("line-calls")).unwrap();
    assert!(calls.lines().any(|line| line.starts_with("started --deadline 4200 ")), "a hold's deadline is cut to the hold ceiling in its line record: {calls}");
    let mut expiring = on_host("1", &header, "");
    let mut input = expiring.stdin.take().unwrap();
    input.write_all(&header).and_then(|()| std::io::copy(&mut fs::File::open(&bundle).unwrap(), &mut input).map(drop)).unwrap();
    let expired = expiring.wait_with_output().unwrap();
    assert_eq!(expired.status.code(), Some(i32::from(super::NO_VERDICT)), "a hold that reached its deadline must read as no verdict, never as a result: {}", String::from_utf8_lossy(&expired.stderr));
    drop(input);
}

#[test]
fn a_job_for_a_host_without_the_lander_runtime_says_so_before_taking_a_slot_and_with_it_runs() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let runtime = "0123456789abcdef0123456789abcdef01234567";
    let job = || {
        Command::new("sh")
            .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", runtime])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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

#[test]
fn a_lane_check_waits_while_the_hosts_admission_refuses_and_a_lander_job_queued_after_it_takes_a_free_slot_first() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let release = home.path().join("release");
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let spawn = |label: &str, lander: &str, hold: bool| {
        let gate = if hold { format!("while [ ! -e {} ]; do sleep 0.02; done", release.display()) } else { ":".to_owned() };
        let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", log = log.display());
        Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", "SAFE_PUSH_LANDING='1'", "", "", "", "", "", lander])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut lane = spawn("lane", "", true);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.contains("start-lane"))), "the admitted lane check never took its slot");
    fs::write(home.path().join("refuse"), "").unwrap();
    let queued = spawn("queued-lane", "", false);
    wait_until_in_line(home.path(), &log, 2);
    let mut holding = spawn("lander", "1", true);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.contains("start-lander"))), "the lander job never took its reserved slot");
    let second = spawn("second-lander", "1", false).wait().unwrap();
    assert!(second.success(), "a lander job queued after the lane check takes a free slot while the host's admission refuses other jobs and the lander's reserved slot is held");
    assert!(!fs::read_to_string(&log).unwrap().contains("start-queued-lane"), "the lane check stays queued while the host's admission refuses, though its checkout has free slots");
    fs::write(&release, "").unwrap();
    assert!(lane.wait().unwrap().success());
    assert!(holding.wait().unwrap().success());
    fs::remove_file(home.path().join("refuse")).unwrap();
    assert!(queued.wait_with_output().unwrap().status.success());
    let order: Vec<String> = fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
    assert_eq!(order.iter().filter(|event| event.contains("lander")).count(), 4, "both lander jobs ran while the admission refused the lane check: {order:?}");
    assert_eq!(order.last().map(String::as_str), Some("end-queued-lane"), "{order:?}");
    a_worker_build_passes_a_land_check_the_host_cannot_lease_yet_which_waits_out_of_a_slot_naming_the_charge();
}

#[test]
fn a_run_whose_connection_is_killed_in_the_middle_still_reports_its_real_exit_and_output_to_a_fresh_connection() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let marker = "north-offload-ran:job-lost";
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let command = "echo before; sleep 2; echo after; exit 3";
    let mut remote = Command::new("setsid")
        .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", "", "", marker])
        .env("HOME", home.path())
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
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
    let ran = super::relay(read.stdout.as_slice(), &mut shown, marker, "", || {}).ran;
    assert_eq!(super::verdict(read.status.code().and_then(|code| u8::try_from(code).ok()), ran), Ok(3), "a fresh connection reads the real exit: {}", String::from_utf8_lossy(&read.stderr));
    assert!(String::from_utf8(shown).unwrap().contains("after"), "a fresh connection reads the output written after the loss");
}

fn relayed_run(home: &Path, head: &str, bundle: &Path, marker: &str, command: &str, grace_seconds: &str) -> std::process::Child {
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    crate::foundation::shim::write_script(&bin.join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    Command::new("setsid")
        .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", head, "30", "code-p-main", command, "refs/land-offload/job", "", "", "", "", marker])
        .env("HOME", home)
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
        .env("NORTH_OFFLOAD_DRAIN_GRACE_SECONDS", grace_seconds)
        .env("PATH", &path)
        .env_remove(north_base::foundation::offload::SLOT_ENV)
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(fs::File::open(bundle).unwrap())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(home.join(format!("{}.err", marker.replace(':', "-")))).unwrap())
        .spawn()
        .unwrap()
}

#[test]
fn a_finished_check_whose_client_stopped_reading_frees_its_slot_while_a_running_check_with_a_slow_client_is_left_alone() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let root = home.path().join(".cache/land-offload");
    let flood = "head -c 300000 /dev/zero | tr '\\0' x; echo";
    let stuck = "north-offload-ran:job-stuck";
    let mut remote = relayed_run(home.path(), &head, &bundle, stuck, &format!("{flood}; exit 3"), "1");
    let unread = remote.stdout.take();
    let exited = root.join("results/job-stuck.exit");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || exited.exists()), "the check never ran");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || remote.try_wait().unwrap().is_some()), "a finished check whose client reads nothing kept its slot");
    assert_eq!(fs::read_to_string(&exited).unwrap().trim(), "3", "the check's exit stays on record for a reattaching client");
    let held = Command::new("sh").arg("-c").arg("for lock in \"$1\"/code-p-main*.lock; do flock -n \"$lock\" true || echo \"$lock\"; done").arg("held").arg(&root).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&held.stdout), "", "a slot stays locked after its relay ended");
    drop(unread);
    let slow = "north-offload-ran:job-slow";
    let mut remote = relayed_run(home.path(), &head, &bundle, slow, &format!("{flood}; sleep 3; touch \"$HOME/unread-past-grace\"; exit 4"), "2");
    let stdout = remote.stdout.take().unwrap();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || home.path().join("unread-past-grace").exists()), "the slow check never reached its end");
    let mut shown = Vec::new();
    let ran = super::relay(std::io::BufReader::new(stdout), &mut shown, slow, "", || {}).ran;
    let code = remote.wait().unwrap().code().and_then(|code| u8::try_from(code).ok());
    assert_eq!(super::verdict(code, ran), Ok(4), "a running check whose client read late lost its verdict: {}", fs::read_to_string(home.path().join("north-offload-ran-job-slow.err")).unwrap_or_default());
    assert!(String::from_utf8(shown).unwrap().contains(&"x".repeat(300_000)), "the slow client read all the output");
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

#[test]
fn a_slot_target_past_its_cap_is_trimmed_and_kept_warm_instead_of_removed() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    let trims = home.path().join("trims");
    let trimmer = format!("if [ \"$1 $2 $3\" = 'capacity offload trim-target' ]; then echo \"$4 $5\" >>{}; rm -rf \"$4/debug/incremental\"; exit 0; fi\n", shell(&trims));
    crate::foundation::shim::write_script(&bin.path().join("north"), &format!("{trimmer}{NORTH_SHIM}"));
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |command: &str| {
        Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", ""])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env("NORTH_OFFLOAD_MAX_JOBS", "1")
            .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
            .env("NORTH_OFFLOAD_TARGET_CAP_MIB", "3")
            .env("NORTH_OFFLOAD_TARGET_KEEP_MIB", "2")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .stdin(fs::File::open(&bundle).unwrap())
            .output()
            .unwrap()
    };
    let built = job("mkdir -p target/debug/deps target/debug/incremental/old && head -c 1048576 /dev/zero >target/debug/deps/libkept.rlib && head -c 4194304 /dev/zero >target/debug/incremental/old/session");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let reused = job("test -f target/debug/deps/libkept.rlib");
    let stderr = String::from_utf8_lossy(&reused.stderr);
    assert!(reused.status.success(), "a slot whose target passed its cap kept the build the next check reuses, not a cold target (north#935): {stderr}");
    let slot = home.path().join(".cache/land-offload/code-p-main/target");
    assert_eq!(fs::read_to_string(&trims).unwrap(), format!("{} 2\n", slot.display()));
}

#[test]
fn a_200_kb_land_check_command_reaches_the_host_intact() {
    let (home, head, bundle_path) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let seen = home.path().join("seen");
    let command = format!("printf 'x' >{0}\n# {1}\nprintf 'y' >>{0}", seen.display(), "z".repeat(200 * 1024));
    let header = bundle::spilled(&command, "SAFE_PUSH_LAND_CPUS='6'", "", "secret", false, &[], fs::metadata(&bundle_path).unwrap().len());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let spilled = header.len().to_string();
    let mut child = super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", "", "refs/land-offload/job", "", "", "", "", "", "", "", &spilled], &[])
        .env("HOME", home.path())
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
        .env("PATH", &path)
        .env_remove("NORTH_OFFLOAD_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let fed = bundle::feed(header, fs::File::open(&bundle_path).unwrap(), child.stdin.take().unwrap(), false);
    let output = child.wait_with_output().unwrap();
    fed.join().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(&seen).unwrap(), "xy");
}

#[test]
fn many_checks_queued_at_once_on_an_idle_host_all_start() {
    let (home, head, _) = host_with_a_shipped_bundle();
    let repo = home.path().join("code/p/main");
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let mut jobs: Vec<std::process::Child> = (0..6)
        .map(|index| {
            let job_ref = format!("refs/land-offload/job-{index}");
            let bundle = home.path().join(format!("job-{index}.bundle"));
            git(&repo, &["update-ref", &job_ref, &head]);
            git(&repo, &["bundle", "create", "-q", bundle.to_str().unwrap(), &job_ref]);
            git(&repo, &["update-ref", "-d", &job_ref]);
            Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", "sleep 8", &job_ref, "SAFE_PUSH_LANDING='1'"])
                .env("HOME", home.path())
                .env("NORTH_OFFLOAD_SETTLE_SECONDS", "30")
                .env("PATH", &path)
                .env_remove(north_base::foundation::offload::SLOT_ENV)
                .env_remove("NORTH_OFFLOAD_ROOT")
                .stdin(fs::File::open(&bundle).unwrap())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let all_ended = north_base::platform::wait::until(std::time::Duration::from_secs(20), || jobs.iter_mut().all(|job| job.try_wait().unwrap().is_some()));
    for job in &mut jobs {
        if job.try_wait().unwrap().is_none() {
            let _ = rustix::process::kill_process_group(rustix::process::Pid::from_raw(job.id() as i32).unwrap(), rustix::process::Signal::Kill);
        }
    }
    let statuses: Vec<_> = jobs.iter_mut().map(|job| job.wait().unwrap()).collect();
    assert!(all_ended && statuses.iter().all(std::process::ExitStatus::success), "six 8 s land checks queued at once on an idle host did not run side by side within 20 s, inside one 30 s settle window: {statuses:?}");
}

fn slot_run(slot: &Path, (base, head): (&str, &str), command: &str) -> Output {
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let home = tempfile::tempdir().unwrap();
    let land = Land { ssh: "", checkout: slot, base: Some(CommitId(base.to_owned())), head: CommitId(head.to_owned()), timeout_seconds: 10, command, class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
    let generated = super::generated::paths(&land);
    let script = format!("set -eu\nslot={} head={} command={} limit=10 generated={}\n{}{}", shell(slot), super::quote(head), super::quote(command), super::quote(&generated), VERIFY.replace("@WRONG_TREE@", &WRONG_TREE.to_string()), run_part());
    Command::new("sh")
        .args(["-c", &script])
        .env("PATH", format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap()))
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .env_remove("SAFE_PUSH_LAND_HEAD")
        .output()
        .unwrap()
}

#[test]
fn a_run_keeps_its_verdict_when_its_build_rewrites_a_declared_generated_file_and_loses_it_when_any_other_tracked_file_changes() {
    let (dir, _, _) = repository_with_two_commits();
    fs::create_dir_all(dir.path().join("harness")).unwrap();
    let declared = "[[set]]\npath = \"listed.rs\"\nkind = \"lines\"\n\n[[set]]\npath = \"registry.txt\"\nkind = \"regenerate\"\ncommand = \"north docs index\"\nsources = [\"registry/\"]\n";
    fs::write(dir.path().join("harness/merge-sets.toml"), declared).unwrap();
    for file in ["registry.txt", "listed.rs", "tracked.rs", "source.rs"] {
        fs::write(dir.path().join(file), "committed\n").unwrap();
    }
    git(dir.path(), &["add", "harness/merge-sets.toml", "registry.txt", "listed.rs", "tracked.rs", "source.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "main declares its registry"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(dir.path().join("harness/merge-sets.toml"), format!("{declared}\n[[set]]\npath = \"source.rs\"\nkind = \"regenerate\"\ncommand = \"north docs index\"\nsources = [\"sources/\"]\n")).unwrap();
    git(dir.path(), &["commit", "-q", "-m", "a lane declares a source file regenerated", "harness/merge-sets.toml"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let lane = (base.as_str(), head.as_str());

    let built = slot_run(dir.path(), lane, "echo built >registry.txt; echo checked");
    assert_eq!(built.status.code(), Some(0), "a build that rewrites the registry its base declares as regenerated keeps its verdict: {}", String::from_utf8_lossy(&built.stderr));
    assert!(String::from_utf8_lossy(&built.stdout).contains("checked"));
    let failed = slot_run(dir.path(), lane, "echo built >registry.txt; exit 3");
    assert_eq!(failed.status.code(), Some(3), "a failing command keeps its own status beside a rewritten registry: {}", String::from_utf8_lossy(&failed.stderr));

    for (file, why_void) in [("tracked.rs", "which no entry declares"), ("listed.rs", "which is declared, but not as regenerated"), ("source.rs", "which only the lane's own commit declares as regenerated")] {
        let void = slot_run(dir.path(), lane, &format!("echo another >{file}"));
        let why = String::from_utf8_lossy(&void.stderr);
        assert_eq!(void.status.code(), Some(i32::from(WRONG_TREE)), "a run that changes {file}, {why_void}, reaches no verdict: {why}");
        assert!(why.contains("the run's verdict is void, its tree changed under it") && why.contains(&format!("M {file}")), "{why}");
    }
    let own = slot_run(dir.path(), (head.as_str(), head.as_str()), "echo built >source.rs");
    assert_eq!(own.status.code(), Some(i32::from(WRONG_TREE)), "a run that names its own head as its base is excused nothing its head declares: {}", String::from_utf8_lossy(&own.stderr));
}

fn a_worker_build_passes_a_land_check_the_host_cannot_lease_yet_which_waits_out_of_a_slot_naming_the_charge() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let release = home.path().join("release");
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let spawn = |label: &str, carried: &str, hold: bool| {
        let gate = if hold { format!("while [ ! -e {} ]; do sleep 0.02; done", release.display()) } else { ":".to_owned() };
        let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", log = log.display());
        Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, "refs/land-offload/job", carried, "", "", ""])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .stdout(Stdio::null())
            .stderr(fs::File::create(home.path().join(format!("{label}.err"))).unwrap())
            .spawn()
            .unwrap()
    };
    let landing = "SAFE_PUSH_LANDING='1'";
    let mut leased = spawn("leased", landing, true);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_to_string(&log).is_ok_and(|text| text.contains("start-leased"))), "the lone land check never took its slot");
    fs::write(home.path().join("landing-full"), "").unwrap();
    let charged = spawn("charged", landing, false);
    wait_until_in_line(home.path(), &log, 2);
    let worker = spawn("worker", "", false).wait().unwrap();
    assert!(worker.success(), "a worker build queued behind a land check the host cannot lease yet takes a free slot");
    assert!(!fs::read_to_string(&log).unwrap().contains("start-charged"), "the land check whose charge does not fit waits in line, out of a slot");
    let waiting = fs::read_to_string(home.path().join("charged.err")).unwrap();
    assert!(waiting.lines().any(|line| line.starts_with("offload: waiting for a slot") && line.contains("land checks charge the cores for the test")), "the wait line says the charge holds it: {waiting}");
    fs::remove_file(home.path().join("landing-full")).unwrap();
    fs::write(&release, "").unwrap();
    assert!(leased.wait().unwrap().success());
    assert!(charged.wait_with_output().unwrap().status.success());
    assert!(fs::read_to_string(&log).unwrap().contains("end-charged"));
}

#[test]
fn a_job_finds_none_of_the_buck2_daemon_state_the_job_before_it_left_for_its_slot_and_refuses_a_live_daemon_or_an_output_it_cannot_remove() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |command: &str| {
        super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", "", "", "", "", ""], &[])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .stdin(fs::File::open(&bundle).unwrap())
            .output()
            .unwrap()
    };
    let state = "\"$HOME/.buck/buckd$(pwd -P)/v2\"";
    let first = job(&format!("mkdir -p {state} && echo first >{state}/buckd.info"));
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let second = job(&format!("test ! -e {state}"));
    assert!(second.status.success(), "the second job in a slot found the Buck2 daemon state the first left, so its buck2 would reach whatever daemon that state names, in the first job's domain: {}", String::from_utf8_lossy(&second.stderr));
    let slot = home.path().canonicalize().unwrap().join(north_base::foundation::offload::ROOT_DEFAULT).join("code-p-main");
    let daemon_state = home.path().join(".buck/buckd").join(slot.strip_prefix("/").unwrap()).join("v2");
    fs::create_dir_all(&daemon_state).unwrap();
    let programs = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&programs.path().join("buck2d"), "while :; do sleep 1; done\n");
    let mut daemon = Command::new(programs.path().join("buck2d")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    fs::write(daemon_state.join("buckd.pid"), format!("{}\n", daemon.id())).unwrap();
    let alive = job("echo ran >&2");
    let _ = daemon.kill();
    let _ = daemon.wait();
    let stderr = String::from_utf8_lossy(&alive.stderr);
    assert!(alive.status.code() == Some(i32::from(WRONG_TREE)) && !stderr.contains("ran\n") && stderr.contains("is still alive"), "a job ran in a slot whose earlier Buck2 daemon was still alive: {stderr}");
    let kept = job("mkdir -p buck-out/v2/kept && touch buck-out/v2/kept/file && chmod 500 buck-out/v2/kept");
    assert!(kept.status.success(), "{}", String::from_utf8_lossy(&kept.stderr));
    let unremoved = job("echo ran >&2");
    let _ = Command::new("chmod").args(["-R", "u+w"]).arg(home.path()).status();
    let stderr = String::from_utf8_lossy(&unremoved.stderr);
    assert!(unremoved.status.code() == Some(i32::from(WRONG_TREE)) && !stderr.contains("ran\n") && stderr.contains("cannot remove the Buck2 output"), "a job ran in a slot that still held the Buck2 output the job before left: {stderr}");
}

#[test]
fn a_closed_host_takes_no_check_and_refuses_its_queue_at_once() {
    let config: toml_edit::DocumentMut = "[hosts.gex45]\nssh = \"gex45\"\noffload_builds = false\n[hosts.cpx62]\nssh = \"cpx62\"\noffload_builds = true\n".parse().unwrap();
    for standing in [super::super::room::Standing::Reserved, super::super::room::Standing::Fallback, super::super::room::Standing::Peer] {
        assert!(!super::super::destinations::takes_land_checks(&config, standing, "gex45"), "placement offers a closed host at {standing:?}");
        assert!(super::super::destinations::takes_land_checks(&config, standing, "cpx62"), "{standing:?}");
    }
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let queue = home.path().join(".cache/land-offload/queue");
    fs::create_dir_all(&queue).unwrap();
    let mut front = Command::new("flock").arg(queue.join("2-1-1@code-p-main")).args(["sleep", "300"]).spawn().unwrap();
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |label: &str| {
        Command::new("sh")
            .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", ":", "refs/land-offload/job", "", "", "", "", "", "", ""])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(fs::File::open(&bundle).unwrap())
            .stderr(fs::File::create(home.path().join(format!("{label}.err"))).unwrap())
            .spawn()
            .unwrap()
    };
    let mut queued: Vec<std::process::Child> = (0..3).map(|index| job(&format!("queued{index}"))).collect();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || (0..3).all(|index| fs::read_to_string(home.path().join(format!("queued{index}.err"))).is_ok_and(|text| text.contains("waiting for a slot")))), "the jobs never waited in line");
    assert!(!north_base::platform::wait::until(std::time::Duration::from_secs(4), || false));
    let asked = fs::read_to_string(home.path().join("admit-calls")).map(|text| text.lines().count()).unwrap_or(0);
    assert!(asked > 0 && asked <= 18, "three waiters behind one ticket asked admission {asked} times in 4 s");
    assert!(queued.iter_mut().all(|waiter| waiter.try_wait().unwrap().is_none()));
    fs::write(home.path().join("closed"), "").unwrap();
    let refused = north_base::platform::wait::until(std::time::Duration::from_secs(10), || queued.iter_mut().all(|waiter| waiter.try_wait().ok().flatten().is_some()));
    let late = job("late");
    let late_status = late_wait(late);
    front.kill().unwrap();
    front.wait().unwrap();
    assert!(refused, "the jobs in line when their host closed were still waiting");
    for (index, waiter) in queued.iter_mut().enumerate() {
        let err = fs::read_to_string(home.path().join(format!("queued{index}.err"))).unwrap();
        assert!(err.contains("gex45 is closed to offloaded checks"), "{err}");
        assert_eq!(waiter.wait().unwrap().code(), Some(i32::from(super::SLOTS_BUSY)));
    }
    assert_eq!(late_status, Some(i32::from(super::SLOTS_BUSY)), "a job placed on the closed host is refused as it arrives");
}

#[test]
fn a_job_whose_host_line_would_keep_it_past_its_deadline_leaves_at_once_with_the_expected_wait_and_a_job_it_admits_records_its_line_history() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let job = |carried: &str| {
        Command::new("sh")
            .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", ":", "refs/land-offload/job", carried, "", "", "", "", "", ""])
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("PATH", &path)
            .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
            .env_remove("NORTH_OFFLOAD_ROOT")
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .stdin(fs::File::open(&bundle).unwrap())
            .output()
            .unwrap()
    };
    fs::write(home.path().join("line-full"), "").unwrap();
    let refused = job("SAFE_PUSH_LANDING='1'");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(i32::from(super::SLOTS_BUSY)), "{told}");
    assert!(told.contains("expected wait 700 s, over the rank's deadline 600 s"), "{told}");
    assert!(!told.contains("waiting for a slot") && !told.contains("running "), "a refused job waited in line or ran: {told}");
    assert_eq!(fs::read_dir(home.path().join(".cache/land-offload/queue")).map(|entries| entries.count()).unwrap_or(0), 0, "a refused job left its ticket in the line");
    assert!(!home.path().join("line-calls").exists(), "a refused job recorded line events for a ticket it never took");
    fs::remove_file(home.path().join("line-full")).unwrap();
    let admitted = job(&format!("SAFE_PUSH_LANDING='1' {}='45'", north_base::foundation::carry::LINE_WITHIN));
    assert!(admitted.status.success(), "{}", String::from_utf8_lossy(&admitted.stderr));
    let asks = fs::read_to_string(home.path().join("line-asks")).unwrap();
    assert!(asks.lines().last().unwrap().starts_with("--rank 2 --ticket ") && asks.lines().last().unwrap().ends_with(&format!(" --name code-p-main --slots {} --within 45", super::SLOTS)), "{asks}");
    let calls = fs::read_to_string(home.path().join("line-calls")).unwrap();
    let events: Vec<&str> = calls.lines().map(|line| line.split(' ').next().unwrap_or("")).collect();
    assert_eq!(events, ["queued", "started", "ended"], "{calls}");
    assert!(calls.lines().all(|line| line.contains("--rank 2 ") && line.contains("--class land")), "{calls}");
    assert!(calls.lines().last().unwrap().contains(" --exit 0 "), "{calls}");
    let markers: Vec<_> = fs::read_dir(home.path().join(".cache/land-offload/inflight")).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(markers.len(), 1, "{markers:?}");
    assert!(Command::new("flock").arg("-n").arg(&markers[0]).arg("true").status().unwrap().success(), "a job that ended still holds its in-flight marker");
    let hosts = || vec![("gex45".to_owned(), "a".to_owned()), ("cpx62".to_owned(), "b".to_owned())];
    assert!(super::twin::targets(None, hosts()).is_empty(), "a run that did not opt in asked a host for a twin");
    assert_eq!(super::twin::targets(Some(""), hosts()), hosts());
    assert_eq!(super::twin::targets(Some("gex45"), hosts()), vec![("cpx62".to_owned(), "b".to_owned())]);
}

fn late_wait(mut job: std::process::Child) -> Option<i32> {
    north_base::platform::wait::until(std::time::Duration::from_secs(10), || job.try_wait().ok().flatten().is_some());
    let status = job.try_wait().ok().flatten().map(|status| status.code());
    job.kill().ok();
    job.wait().ok();
    status.flatten()
}

const SAME_HOST_HOME: &str = "NORTH_TEST_SAME_HOST_HOME";
const SAME_HOST_HEAD: &str = "NORTH_TEST_SAME_HOST_HEAD";
const SAME_HOST_BUNDLE: &str = "NORTH_TEST_SAME_HOST_BUNDLE";
const SAME_HOST_PATH: &str = "NORTH_TEST_SAME_HOST_PATH";

fn same_host_job(home: &Path, head: &str, bundle: &Path, path: &str) -> Output {
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    super::on_host("", &script, &["code/p/main", "", head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", "", "", "", "", ""], &[])
        .env("HOME", home)
        .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
        .env("PATH", path)
        .env("NORTH_OFFLOAD_MAX_JOBS", "1")
        .env("AGENT_CAPACITY_DISK_FREE_GIB", "1000")
        .env_remove("NORTH_OFFLOAD_ROOT")
        .env_remove(north_base::foundation::offload::SLOT_ENV)
        .stdin(fs::File::open(bundle).unwrap())
        .output()
        .unwrap()
}

#[test]
fn a_job_for_this_host_started_with_a_lanes_git_variables_leaves_that_lane_on_its_branch_with_its_uncommitted_and_untracked_files() {
    if let Some(home) = std::env::var_os(SAME_HOST_HOME) {
        let read = |name: &str| std::env::var(name).unwrap();
        let ran = same_host_job(Path::new(&home), &read(SAME_HOST_HEAD), Path::new(&read(SAME_HOST_BUNDLE)), &read(SAME_HOST_PATH));
        assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
        return;
    }
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let lane = tempfile::tempdir().unwrap();
    git(lane.path(), &["init", "-q", "-b", "work"]);
    fs::write(lane.path().join("kept"), "committed\n").unwrap();
    git(lane.path(), &["add", "kept"]);
    git(lane.path(), &["commit", "-q", "-m", "lane"]);
    fs::write(lane.path().join("kept"), "uncommitted\n").unwrap();
    fs::write(lane.path().join("untracked"), "new\n").unwrap();
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let placed = same_host_job(home.path(), &head, &bundle, &path);
    assert!(placed.status.success(), "{}", String::from_utf8_lossy(&placed.stderr));
    let inherited = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "land_offload::ship::tests::a_job_for_this_host_started_with_a_lanes_git_variables_leaves_that_lane_on_its_branch_with_its_uncommitted_and_untracked_files", "--nocapture"])
        .env(SAME_HOST_HOME, home.path())
        .env(SAME_HOST_HEAD, &head)
        .env(SAME_HOST_BUNDLE, &bundle)
        .env(SAME_HOST_PATH, &path)
        .env("GIT_DIR", lane.path().join(".git"))
        .env("GIT_COMMON_DIR", lane.path().join(".git"))
        .env("GIT_WORK_TREE", lane.path())
        .output()
        .unwrap();
    let child = String::from_utf8_lossy(&inherited.stdout);
    assert!(inherited.status.success() && child.contains(" 1 passed"), "the child runs this one test: {child}");
    assert_eq!(git(lane.path(), &["symbolic-ref", "--short", "HEAD"]), "work", "the slot's checkout never moves the lane its caller's git variables name");
    assert_eq!(fs::read_to_string(lane.path().join("kept")).unwrap(), "uncommitted\n");
    assert_eq!(fs::read_to_string(lane.path().join("untracked")).unwrap(), "new\n");
}

fn remote_script(name: &str, class: super::SlotClass) -> String {
    let script = super::remote_script(name, class);
    assert!(script.contains("operator_file=/etc/north/operator"), "fill.sh no longer names the operator file the ship tests replace");
    script.replace("operator_file=/etc/north/operator", "operator_file=/nonexistent/north-operator")
}

#[test]
fn a_job_whose_client_is_killed_leaves_its_slot_and_its_ticket_whether_it_holds_a_slot_or_waits_while_one_whose_connection_dropped_waits_for_its_client_to_reattach() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let root = home.path().join(".cache/land-offload");
    let bin = tempfile::tempdir().unwrap();
    crate::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let connection = bin.path().join("sshd-session");
    crate::foundation::shim::write_script(&connection, "cat | \"$@\"\n");
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script("code-p-main", super::SlotClass::LandCheck);
    let bundled = fs::metadata(&bundle).unwrap().len();
    let release = home.path().join("release");
    let start = |label: &str, through: Option<&Path>| {
        let pid = home.path().join(format!("{label}.pid"));
        let header = bundle::spilled(&format!("echo $$ >{}; while [ ! -e {} ]; do sleep 0.1; done; exit 3", pid.display(), release.display()), "", "", "secret", false, &[], bundled);
        let spilled = header.len().to_string();
        let header_path = home.path().join(format!("{label}.header"));
        fs::write(&header_path, header).unwrap();
        let mut client = Command::new("setsid").args(["sh", "-c", "cat \"$1\" \"$2\"; exec sleep 600", "client"]).arg(&header_path).arg(&bundle).stdout(Stdio::piped()).spawn().unwrap();
        let marker = format!("north-offload-ran:job-{label}");
        let words: [&str; 15] = ["code/p/main", "", &head, "30", "code-p-main", "", "refs/land-offload/job", "", "", "", "", &marker, "", "", &spilled];
        let mut job = match through {
            Some(connection) => {
                let mut job = Command::new(connection);
                job.args(["setsid", "-w", "sh", "-c", &script, "offload"]).args(words);
                std::os::unix::process::CommandExt::process_group(&mut job, 0);
                job
            }
            None => super::on_host("", &script, &words, &[]),
        };
        let job = job
            .env("HOME", home.path())
            .env("NORTH_OFFLOAD_SETTLE_SECONDS", "0")
            .env("NORTH_OFFLOAD_MAX_JOBS", "1")
            .env("PATH", &path)
            .env_remove(north_base::foundation::offload::SLOT_ENV)
            .env_remove("NORTH_OFFLOAD_ROOT")
            .stdin(Stdio::from(client.stdout.take().unwrap()))
            .stdout(Stdio::null())
            .stderr(fs::File::create(home.path().join(format!("{label}.err"))).unwrap())
            .spawn()
            .unwrap();
        (client, job, pid, marker)
    };
    let kill_group = |child: &std::process::Child| {
        let _ = rustix::process::kill_process_group(rustix::process::Pid::from_raw(child.id() as i32).unwrap(), rustix::process::Signal::Kill);
    };
    let alive = |pid: &Path| fs::read_to_string(pid).ok().and_then(|pid| pid.trim().parse::<i32>().ok()).and_then(rustix::process::Pid::from_raw).is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok());
    let (mut holder_client, mut holder, holder_pid, _) = start("holder", None);
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || holder_pid.exists()), "the first job never ran: {}", fs::read_to_string(home.path().join("holder.err")).unwrap_or_default());
    let (mut waiter_client, mut waiter, waiter_pid, _) = start("waiter", None);
    let queue = root.join("queue");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || fs::read_dir(&queue).map(|entries| entries.count()).unwrap_or(0) == 1), "the second job never queued: {}", fs::read_to_string(home.path().join("waiter.err")).unwrap_or_default());
    for client in [&mut holder_client, &mut waiter_client] {
        kill_group(client);
        client.wait().unwrap();
    }
    let held = || Command::new("sh").arg("-c").arg("for lock in \"$1\"/code-p-main*.lock; do flock -n \"$lock\" true || echo \"$lock\"; done").arg("held").arg(&root).output().unwrap().stdout;
    let freed = north_base::platform::wait::until(std::time::Duration::from_secs(28), || {
        holder.try_wait().unwrap().is_some() && waiter.try_wait().unwrap().is_some() && !alive(&holder_pid) && held().is_empty() && fs::read_dir(&queue).unwrap().count() == 0
    });
    let left = (holder.try_wait().unwrap(), waiter.try_wait().unwrap(), alive(&holder_pid), String::from_utf8_lossy(&held()).into_owned(), fs::read_dir(&queue).unwrap().count());
    for job in [&mut holder, &mut waiter] {
        kill_group(job);
        let _ = job.wait();
    }
    assert!(freed, "28 s after their clients were killed (holder exit, waiter exit, check alive, held locks, tickets): {left:?}");
    assert!(!waiter_pid.exists(), "the job that waited ran its check after its client was gone");
    let (mut dropped_client, mut dropped, dropped_pid, marker) = start("dropped", Some(&connection));
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || dropped_pid.exists()), "the third job never ran: {}", fs::read_to_string(home.path().join("dropped.err")).unwrap_or_default());
    kill_group(&dropped);
    let _ = dropped.wait();
    let survived = !north_base::platform::wait::until(std::time::Duration::from_secs(29), || !alive(&dropped_pid));
    let reattached = Command::new("sh").args(["-c", &super::script::reattach_script(), "offload-reattach", &marker, "30"]).env("HOME", home.path()).env_remove("NORTH_OFFLOAD_ROOT").stdout(Stdio::piped()).spawn().unwrap();
    fs::write(&release, "").unwrap();
    let read = reattached.wait_with_output().unwrap();
    kill_group(&dropped_client);
    let _ = dropped_client.wait();
    assert!(survived, "a running job whose connection dropped while its client lived ended before the client's reattach window: {}", fs::read_to_string(home.path().join("dropped.err")).unwrap_or_default());
    let ran = super::relay(read.stdout.as_slice(), &mut Vec::new(), &marker, "", || {}).ran;
    assert_eq!(super::verdict(read.status.code().and_then(|code| u8::try_from(code).ok()), ran), Ok(3), "the reattached client reads the check's own exit");
}
