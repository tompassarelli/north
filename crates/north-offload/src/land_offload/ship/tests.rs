pub(super) use north_base::test_inputs::git::git_in as git;
use super::script::{VERIFY, run_part};
use super::{CommitId, Land, WRONG_TREE, bundle, carried, job_ref};
use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn carried_from(caller: &[(&str, &str)]) -> String {
    let land = Land { ssh: "", checkout: Path::new("."), base: Some(CommitId("b".to_owned())), head: CommitId("h".to_owned()), timeout_seconds: 10, command: "", outputs: &[], class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
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
  "depbox check") shift 3; echo "$*" >>"$HOME/depbox-checks"; exec "$@" ;;
  "--worker-sandbox --candidate") shift 3; echo "$*" >>"$HOME/candidate-checks"; exec "$@" ;;
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

const LANDER_RUNTIME: &str = "0123456789abcdef0123456789abcdef01234567";

fn lander_runtime(home: &Path) {
    let runtime = home.join(".local/state/north/runtimes").join(LANDER_RUNTIME);
    let marker = runtime.join(north_runtime_follow::layout::tree_marker());
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(marker, "").unwrap();
    north_base::foundation::shim::write_script(&runtime.join("north"), NORTH_SHIM);
}

fn remote_run_flake(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>) -> String {
    remote_run(command, flake, caller, home).0
}

fn remote_run(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>) -> (String, String) {
    remote_run_after(command, flake, caller, home, "")
}

fn remote_run_after(command: &str, flake: Option<&str>, caller: &[(&str, &str)], home: Option<&Path>, prelude: &str) -> (String, String) {
    let slot = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    if let Some(text) = flake {
        fs::write(slot.path().join("flake.nix"), text).unwrap();
    }
    let nix = bin.path().join("nix");
    north_base::foundation::shim::write_script(&nix, NIX_SHIM);
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let fresh_home = tempfile::tempdir().unwrap();
    lander_runtime(home.unwrap_or(fresh_home.path()));
    let output = Command::new("sh")
        .args(["-c", &format!("set -eu\nslot={} command={} runtime={LANDER_RUNTIME} limit=10 base=b head=h carried={}\n{prelude}proved() {{ :; }}\n{}", shell(slot.path()), shell(Path::new(command)), super::quote(&carried_from(caller)), run_part())])
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

fn repository_with_two_commits() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "first"]);
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "second"]);
    let second = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, first, second)
}

fn reap_script(root: &Path, owner: &str) -> Output {
    let script = super::REAP.replace("@ROOT@", &shell(root)).replace("sleep 2\n", "sleep 0\n");
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
    let home = tempfile::tempdir().unwrap();
    remote_run("true", None, &[], Some(home.path()));
    remote_run_after("true", None, &[], Some(home.path()), "holding=1; ");
    let confined = fs::read_to_string(home.path().join("candidate-checks")).unwrap_or_default();
    assert_eq!(confined.lines().filter(|line| line.contains("depbox check -- /") && line.ends_with("/bash -euo pipefail /dev/fd/7")).count(), 2, "a lane check on a lander host ran outside the candidate jail, held or not (south#138): {confined:?}");
}

const LANE_CHECKOUT: &str = "NORTH_TEST_LANE_CHECKOUT";

#[test]
fn a_job_bundles_when_its_caller_names_its_lane_in_git_work_tree_as_a_finish_check_does() {
    if let Some(checkout) = std::env::var_os(LANE_CHECKOUT) {
        let checkout = Path::new(&checkout);
        let head = CommitId::resolve(checkout, "HEAD").unwrap();
        let reference = job_ref(&head);
        let land = Land { ssh: "", checkout, base: None, head: head.clone(), timeout_seconds: 1, command: "", outputs: &[], class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
        let listed = Command::new("git").current_dir(checkout).args(["bundle", "list-heads", "-"]).stdin(bundle(&land, &reference, None).unwrap()).output().unwrap();
        assert_eq!(String::from_utf8(listed.stdout).unwrap().trim(), format!("{} {reference}", head.as_str()));
        return;
    }
    let (dir, _, _) = repository_with_two_commits();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "land_offload::ship::tests::a_job_bundles_when_its_caller_names_its_lane_in_git_work_tree_as_a_finish_check_does", "--nocapture"])
        .env(LANE_CHECKOUT, dir.path())
        .env("GIT_WORK_TREE", dir.path())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&child.stdout);
    assert!(child.status.success() && out.contains(" 1 passed"), "{out}{}", String::from_utf8_lossy(&child.stderr));
}

#[test]
fn a_job_bundles_from_a_checkout_whose_git_directory_is_read_only_as_inside_a_land_sandbox() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, first, _) = repository_with_two_commits();
    let head = CommitId::resolve(dir.path(), "HEAD").unwrap();
    let base = CommitId::resolve(dir.path(), &first).unwrap();
    let refs = dir.path().join(".git/refs");
    let locked = |mode| fs::set_permissions(&refs, fs::Permissions::from_mode(mode)).unwrap();
    locked(0o555);
    let reference = job_ref(&head);
    let land = Land { ssh: "", checkout: dir.path(), base: Some(base), head: head.clone(), timeout_seconds: 1, command: "", outputs: &[], class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
    let bundled = bundle(&land, &reference, None);
    locked(0o755);
    let listed = Command::new("git").current_dir(dir.path()).args(["bundle", "list-heads", "-"]).stdin(bundled.unwrap()).output().unwrap();
    assert_eq!(String::from_utf8(listed.stdout).unwrap().trim(), format!("{} {reference}", head.as_str()));
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
    let script = super::script::slot_part().replace("@JOBS@", &super::script::jobs_part()).replace("@ADMIT@", super::script::ADMIT).replace("@SETTLE@", "0").replace("@ROOT@", super::OFFLOAD_ROOT).replace("@LINE@", "lock_wait=0\nline_note() { :; }\n").replace("@SLOTS_BUSY@", "96").replace("@SLOTS@", "1");
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
    north_base::foundation::shim::write_script(&north, "printf '%s|%s\\n' \"$PWD\" \"$*\" >>\"$HOME/asked\"\n");
    let output = ship(Some(&north));
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{told}");
    let asked = fs::read_to_string(home.path().join("asked")).unwrap_or_default();
    assert!(asked.lines().any(|line| line == format!("{}|capacity offload slot record code-p-main", repo.display())), "a script that could not write its slot's record, as one run inside a session's domain cannot, asked north for no record from the main checkout, so the broker refuses every request its job makes: north was asked {asked:?}; {told}");
    assert!(!told.contains("cannot record"), "{told}");
    let older = home.path().join("older");
    north_base::foundation::shim::write_script(&older, "echo 'usage: north capacity offload' >&2\nexit 2\n");
    let current = home.path().join(".local/state/north/current/north");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_script(&current, "printf '%s|%s\\n' \"$PWD\" \"$*\" >>\"$HOME/asked-current\"\n");
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
    lander_runtime(home.path());
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
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let log = home.path().join("order.log");
    let children: Vec<std::process::Child> = jobs
        .iter()
        .enumerate()
        .map(|(index, job)| {
            let name = job.class.slot_name(job.name);
            let script = remote_script(job.class);
            let gate = if job.hold { format!("while [ ! -e {}/release-{} ]; do sleep 0.02; done", home.path().display(), job.label) } else { ":".to_owned() };
            let command = format!("echo start-{label} >>{log}; {gate}; echo end-{label} >>{log}", label = job.label, log = log.display());
            let child = Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", &name, &command, "refs/land-offload/job", job.carried, job.owner, LANDER_RUNTIME, job.red_fix, "", "", "", "", "", job.urgent])
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
fn a_red_main_fix_takes_the_reserved_slot_beside_a_held_one_then_a_lander_placed_hand_in_takes_the_first_freed_slot_ahead_of_earlier_worker_builds_while_a_background_sweep_holds_none_of_their_slots() {
    let landing = "SAFE_PUSH_LANDING='1'";
    let order = order_through_one_slot(&[
        Queued { label: "sweep", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: true, class: super::SlotClass::Background },
        Queued { label: "holder", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: true, class: super::SlotClass::LandCheck },
        Queued { label: "worker", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "forged-land", name: "code-p-main", carried: landing, red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "land", name: "code-p-main", carried: landing, red_fix: "", owner: "land-check", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "red-fix", name: "code-p-main", carried: landing, red_fix: "1", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "forged", name: "code-p-main", carried: "SAFE_PUSH_LANDING='after'", red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "after", name: "code-p-main", carried: "SAFE_PUSH_LANDING='after'", red_fix: "", owner: "land-check", urgent: "", hold: false, class: super::SlotClass::LandCheck },
    ]);
    let foreground: Vec<&str> = order.iter().map(String::as_str).filter(|event| *event != "start-sweep").collect();
    assert_eq!(foreground, ["start-holder", "start-red-fix", "end-red-fix", "end-holder", "start-land", "end-land", "start-after", "end-after", "start-worker", "end-worker", "start-forged-land", "end-forged-land", "start-forged", "end-forged", "end-sweep"], "the red fix took the reserved slot beside the holder at once, the lander's hand-in land check next and its after-landing job queued after it at the same rank after that, which leaves the lander's first-in-line rank to its pre-push checks, then the worker builds in arrival order with the hand-in word that carries no lander owner among them, and the sweep outlasted them all");
    let at = |event: &str| order.iter().position(|seen| seen == event);
    assert!(at("start-sweep") < at("end-holder"), "the sweep ran in its own slot beside the holder, outside the one-job cap: {order:?}");
    let urgent = order_through_one_slot(&[
        Queued { label: "holder", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: true, class: super::SlotClass::LandCheck },
        Queued { label: "normal", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "", hold: false, class: super::SlotClass::LandCheck },
        Queued { label: "urgent", name: "code-p-main", carried: "", red_fix: "", owner: "", urgent: "1", hold: false, class: super::SlotClass::LandCheck },
    ]);
    assert_eq!(urgent, ["start-holder", "end-holder", "start-urgent", "end-urgent", "start-normal", "end-normal"], "the urgent session's job, queued behind a normal one, took the freed slot first: {urgent:?}");
}

const PROVED: &str = "0123456789abcdef0123456789abcdef01234567";

fn remote_reports(command: &str, marker: &str) -> (Option<u8>, Option<u8>, String) {
    let slot = tempfile::tempdir().unwrap();
    lander_runtime(slot.path());
    let script = format!("set -eu\nslot={} command={} limit=10 ran={} runtime={LANDER_RUNTIME} proof=secret proved_tree={PROVED}\nproved() {{ :; }}\n{}", shell(slot.path()), super::quote(command), super::quote(marker), run_part());
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
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
fn a_slot_job_that_offloads_back_to_its_own_host_runs_the_nested_job_without_waiting_out_the_queue_and_ends_it_with_its_check_and_a_held_slot_takes_only_jobs_with_its_token_while_its_caller_lives() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let repo = home.path().join("code/p/main");
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let other = git(&repo, &["rev-parse", "HEAD"]);
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
    let inner = home.path().join("inner.sh");
    fs::write(&inner, &script).unwrap();
    let nested = |head: &str, ran: &Path, then: &str| format!("setsid -w sh -c \"$(cat {inner})\" offload code/p/main '' {head} 30 code-p-main 'printf %s \"${slot_env}\" >{ran}{then}' refs/land-offload/job '' '' {LANDER_RUNTIME} '' '' '' 0 <{bundle}", inner = inner.display(), ran = ran.display(), bundle = bundle.display(), slot_env = north_base::foundation::offload::SLOT_ENV);
    let job = |command: &str, cap: &str, log: &str| {
        let mut outer = Command::new("setsid")
            .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", LANDER_RUNTIME])
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
    let linger = format!("setsid -w sh -c \"$(cat {inner})\" offload code/p/main '' {head} 30 code-p-main 'printf %s \"${slot_env}\" >{ran}; sleep 30; : >{ended}' refs/land-offload/job '' '' {LANDER_RUNTIME} '' north-offload-ran:linger <{bundle} >/dev/null 2>{said} & until [ -e {ran} ]; do sleep 0.05; done", inner = inner.display(), ran = lingering.display(), ended = ended.display(), said = said.display(), bundle = bundle.display(), slot_env = north_base::foundation::offload::SLOT_ENV);
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
        super::on_host("", &script, &["code/p/main", "", &head, limit, "code-p-main", "", "refs/land-offload/job", "", "", LANDER_RUNTIME, "", "", "", lander_slot, &header.len().to_string()], &[])
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
    let mut expiring = on_host("0", &header, "");
    let mut input = expiring.stdin.take().unwrap();
    input.write_all(&header).and_then(|()| std::io::copy(&mut fs::File::open(&bundle).unwrap(), &mut input).map(drop)).unwrap();
    let expired = expiring.wait_with_output().unwrap();
    assert_eq!(expired.status.code(), Some(i32::from(super::NO_VERDICT)), "a hold that reached its deadline must read as no verdict, never as a result: {}", String::from_utf8_lossy(&expired.stderr));
    drop(input);
}

#[test]
fn many_checks_queued_at_once_on_an_idle_host_all_start() {
    let (home, head, _) = host_with_a_shipped_bundle();
    let repo = home.path().join("code/p/main");
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
    let started = home.path().join("started");
    fs::create_dir(&started).unwrap();
    let side_by_side = super::SLOTS - 1;
    let command = format!("mktemp {0}/XXXXXX >/dev/null; until [ \"$(ls {0} | wc -l)\" -ge {side_by_side} ]; do sleep 0.02; done", super::quote(&started.display().to_string()));
    let mut jobs: Vec<std::process::Child> = (0..6)
        .map(|index| {
            let job_ref = format!("refs/land-offload/job-{index}");
            let bundle = home.path().join(format!("job-{index}.bundle"));
            git(&repo, &["update-ref", &job_ref, &head]);
            git(&repo, &["bundle", "create", "-q", bundle.to_str().unwrap(), &job_ref]);
            git(&repo, &["update-ref", "-d", &job_ref]);
            Command::new("setsid")
                .args(["-w", "sh", "-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", &command, &job_ref, "SAFE_PUSH_LANDING='1'", "land-check", LANDER_RUNTIME])
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
    assert!(all_ended && statuses.iter().all(std::process::ExitStatus::success), "six land checks queued at once on an idle host did not run {side_by_side} side by side within 20 s, inside one 30 s settle window ({} started): {statuses:?}", fs::read_dir(&started).unwrap().count());
}

fn slot_run(slot: &Path, (base, head): (&str, &str), command: &str) -> Output {
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let home = tempfile::tempdir().unwrap();
    lander_runtime(home.path());
    let land = Land { ssh: "", checkout: slot, base: Some(CommitId(base.to_owned())), head: CommitId(head.to_owned()), timeout_seconds: 10, command, outputs: &[], class: super::SlotClass::LandCheck, confined: false, hold: super::Hold::None };
    let generated = super::generated::paths(&land);
    let script = format!("set -eu\nslot={} head={} command={} limit=10 generated={} runtime={LANDER_RUNTIME}\n{}{}", shell(slot), super::quote(head), super::quote(command), super::quote(&generated), VERIFY.replace("@WRONG_TREE@", &WRONG_TREE.to_string()), run_part());
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
    let declared = "[[set]]\npath = \"listed.rs\"\nkind = \"lines\"\n\n[[set]]\npath = \"registry.txt\"\nkind = \"regenerate\"\ncommand = \"north docs index\"\nsources = [\"registry/\"]\nbuilt = true\n";
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

#[test]
fn a_job_finds_none_of_the_buck2_daemon_state_the_job_before_it_left_for_its_slot_and_refuses_a_live_daemon_or_an_output_it_cannot_remove() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
    let job = |command: &str| {
        super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", command, "refs/land-offload/job", "", "", LANDER_RUNTIME, "", "", "", ""], &[])
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
    north_base::foundation::shim::write_script(&programs.path().join("buck2d"), "while :; do sleep 1; done\n");
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
fn a_job_whose_host_line_would_keep_it_past_its_deadline_leaves_at_once_with_the_expected_wait_and_a_job_it_admits_records_its_line_history() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
    let job = |carried: &str| {
        Command::new("sh")
            .args(["-c", &script, "offload", "code/p/main", "", &head, "30", "code-p-main", ":", "refs/land-offload/job", carried, "land-check", LANDER_RUNTIME, "", "", "", "", ""])
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
    assert!(asks.lines().last().unwrap().starts_with("--rank 2 --ticket ") && asks.lines().last().unwrap().ends_with(&format!(" --name code-p-main --slots {} --within 45", super::SLOTS - 1)), "a job other than the lander's waits for one of the slots its reserved one leaves: {asks}");
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

const SAME_HOST_HOME: &str = "NORTH_TEST_SAME_HOST_HOME";
const SAME_HOST_HEAD: &str = "NORTH_TEST_SAME_HOST_HEAD";
const SAME_HOST_BUNDLE: &str = "NORTH_TEST_SAME_HOST_BUNDLE";
const SAME_HOST_PATH: &str = "NORTH_TEST_SAME_HOST_PATH";

fn same_host_job(home: &Path, head: &str, bundle: &Path, path: &str) -> Output {
    let script = remote_script(super::SlotClass::LandCheck);
    super::on_host("", &script, &["code/p/main", "", head, "30", "code-p-main", "true", "refs/land-offload/job", "", "", LANDER_RUNTIME, "", "", "", ""], &[])
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
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
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

fn remote_script(class: super::SlotClass) -> String {
    let script = super::remote_script(class);
    assert!(script.contains("operator_file=/etc/north/operator"), "fill.sh no longer names the operator file the ship tests replace");
    script.replace("operator_file=/etc/north/operator", "operator_file=/nonexistent/north-operator")
}

#[test]
fn a_job_whose_client_is_killed_leaves_its_slot_and_its_ticket_whether_it_holds_a_slot_or_waits_while_one_whose_connection_dropped_waits_for_its_client_to_reattach() {
    let (home, head, bundle) = host_with_a_shipped_bundle();
    let root = home.path().join(".cache/land-offload");
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let connection = bin.path().join("sshd-session");
    north_base::foundation::shim::write_script(&connection, "cat | \"$@\"\n");
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
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
        let words: [&str; 15] = ["code/p/main", "", &head, "30", "code-p-main", "", "refs/land-offload/job", "", "", LANDER_RUNTIME, "", &marker, "", "", &spilled];
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
            .env("NORTH_OFFLOAD_TETHER_SECONDS", "3")
            .env("NORTH_OFFLOAD_DROPPED_SECONDS", "12")
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
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(3600), || alive(&holder_pid)), "the first job never ran: {}", fs::read_to_string(home.path().join("holder.err")).unwrap_or_default());
    let (mut waiter_client, mut waiter, waiter_pid, _) = start("waiter", None);
    let queue = root.join("queue");
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(3600), || fs::read_dir(&queue).map(|entries| entries.count()).unwrap_or(0) == 1), "the second job never queued: {}", fs::read_to_string(home.path().join("waiter.err")).unwrap_or_default());
    for client in [&mut holder_client, &mut waiter_client] {
        kill_group(client);
        client.wait().unwrap();
    }
    let held = || Command::new("sh").arg("-c").arg("for lock in \"$1\"/code-p-main*.lock; do flock -n \"$lock\" true || echo \"$lock\"; done").arg("held").arg(&root).output().unwrap().stdout;
    let freed = north_base::platform::wait::until(std::time::Duration::from_secs(3600), || {
        holder.try_wait().unwrap().is_some() && waiter.try_wait().unwrap().is_some() && !alive(&holder_pid) && held().is_empty() && fs::read_dir(&queue).unwrap().count() == 0
    });
    let left = (holder.try_wait().unwrap(), waiter.try_wait().unwrap(), alive(&holder_pid), String::from_utf8_lossy(&held()).into_owned(), fs::read_dir(&queue).unwrap().count());
    for job in [&mut holder, &mut waiter] {
        kill_group(job);
        let _ = job.wait();
    }
    assert!(freed, "after their clients were killed (holder exit, waiter exit, check alive, held locks, tickets): {left:?}");
    assert!(!waiter_pid.exists(), "the job that waited ran its check after its client was gone");
    let (mut dropped_client, mut dropped, dropped_pid, marker) = start("dropped", Some(&connection));
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(3600), || alive(&dropped_pid)), "the third job never ran: {}", fs::read_to_string(home.path().join("dropped.err")).unwrap_or_default());
    kill_group(&dropped);
    let _ = dropped.wait();
    let survived = !north_base::platform::wait::until(std::time::Duration::from_secs(8), || !alive(&dropped_pid));
    let reattached = Command::new("sh").args(["-c", &super::script::reattach_script(), "offload-reattach", &marker, "30"]).env("HOME", home.path()).env_remove("NORTH_OFFLOAD_ROOT").stdout(Stdio::piped()).spawn().unwrap();
    fs::write(&release, "").unwrap();
    let read = reattached.wait_with_output().unwrap();
    kill_group(&dropped_client);
    let _ = dropped_client.wait();
    assert!(survived, "a running job whose connection dropped while its client lived ended before the client's reattach window: {}", fs::read_to_string(home.path().join("dropped.err")).unwrap_or_default());
    let ran = super::relay(read.stdout.as_slice(), &mut Vec::new(), &marker, "", || {}).ran;
    assert_eq!(super::verdict(read.status.code().and_then(|code| u8::try_from(code).ok()), ran), Ok(3), "the reattached client reads the check's own exit");
}

#[test]
fn an_offload_header_word_that_would_run_a_command_is_refused_without_running_it_north_1041() {
    let (home, head, bundle_path) = host_with_a_shipped_bundle();
    let bin = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&bin.path().join("north"), NORTH_SHIM);
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
    let script = remote_script(super::SlotClass::LandCheck);
    let bundled = fs::metadata(&bundle_path).unwrap().len();
    let ran = home.path().join("ran");
    let planted = |name: &str| home.path().join(name);
    let ship = |header: Vec<u8>| {
        let spilled = header.len().to_string();
        let mut child = super::on_host("", &script, &["code/p/main", "", &head, "30", "code-p-main", "", "refs/land-offload/job", "", "", LANDER_RUNTIME, "", "", "", "", &spilled], &[])
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
        output
    };
    let steered = tempfile::tempdir().unwrap();
    north_base::foundation::shim::write_script(&steered.path().join("north"), &format!("#!/bin/sh\ntouch {}\n", planted("path").display()));
    let command = format!("touch {}", ran.display());
    let headers = [
        ("proof", format!("command={}\nproof=$(touch {})\n", super::quote(&command), planted("proof").display()).into_bytes()),
        ("carried", bundle::spilled(&command, &format!("SAFE_PUSH_LAND_CPUS=$(touch {})", planted("carried").display()), "", "secret", false, &[], bundled)),
        ("path", bundle::spilled(&command, &format!("PATH={}", super::quote(&format!("{}:{path}", steered.path().display()))), "", "secret", false, &[], bundled)),
        ("preload", bundle::spilled(&command, &format!("LD_PRELOAD={}", super::quote(&planted("preload.so").display().to_string())), "", "secret", false, &[], bundled)),
    ];
    for (name, header) in headers {
        let output = ship(header);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!planted(name).exists(), "a command in the header's {name} ran on the host: {stderr}");
        assert!(!output.status.success() && stderr.contains("refusing to run") && !ran.exists(), "a header whose {name} would run a command was not refused: {stderr}");
    }
}

fn verify(slot: &Path, head: &str) -> Output {
    let script = VERIFY.replace("@WRONG_TREE@", &WRONG_TREE.to_string());
    Command::new("sh").args(["-c", &format!("set -eu\nslot={} head={}\n{script}", shell(slot), super::quote(head))]).output().unwrap()
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

