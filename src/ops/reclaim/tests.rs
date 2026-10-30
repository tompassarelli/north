use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, flock};

use super::{Inputs, Policy, reclaim, summary};
use crate::platform::disk_budget::DiskBudget;

pub(super) const DAY: Duration = Duration::from_secs(24 * 3600);

pub(super) struct Machine {
    root: tempfile::TempDir,
    pub(super) now: SystemTime,
}

impl Machine {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["tmp", "proc", "clients", "state", "code", "cache"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        Self { root, now: SystemTime::now() + 2 * DAY }
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    pub(super) fn dir(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).unwrap();
        touch(&path, self.now - age);
        path
    }

    fn file(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        touch(&path, self.now - age);
        path
    }

    pub(super) fn inputs(&self) -> Inputs {
        Inputs {
            tmp: self.path("tmp"),
            proc_root: self.path("proc"),
            code: self.path("code"),
            cache: self.path("cache"),
            lan_clients: self.path("clients"),
            lan_state: self.path("state"),
            evidence: self.path("evidence"),
            disk_budget: DiskBudget::default(),
            open_issue_text: || Ok(String::new()),
            uid: rustix::process::getuid().as_raw(),
            now: self.now,
        }
    }

    fn reclaim(&self, dry_run: bool) -> Vec<PathBuf> {
        let removals = reclaim(&self.inputs(), &Policy { idle: DAY, dry_run }).unwrap();
        let mut paths = removals.into_iter().map(|removal| removal.path).collect::<Vec<_>>();
        paths.sort();
        paths
    }
}

pub(super) fn touch(path: &Path, at: SystemTime) {
    File::open(path).unwrap().set_modified(at).unwrap();
}

#[test]
fn scratch_goes_once_idle_past_the_limit_and_held_by_no_process() {
    let machine = Machine::new();
    let abandoned = machine.dir("tmp/sc420", 3 * DAY);
    let abandoned_file = machine.file("tmp/sc50-native.png", 3 * DAY);
    let fresh = machine.dir("tmp/sc509", Duration::from_secs(600));
    let working = machine.dir("tmp/sc363", 3 * DAY);
    machine.file("tmp/sc363/frame.png", Duration::from_secs(60));
    touch(&working, machine.now - 3 * DAY);
    let held = machine.dir("tmp/sc434", 3 * DAY);
    machine.dir("proc/4242/fd", DAY);
    symlink(held.join("out.log"), machine.path("proc/4242/fd/3")).unwrap();
    let display = machine.file("tmp/.X1-lock", 3 * DAY);
    let multiplexer = machine.dir("tmp/tmux-1000", 3 * DAY);
    std::os::unix::net::UnixListener::bind(multiplexer.join("default")).unwrap();
    touch(&multiplexer, machine.now - 3 * DAY);

    assert_eq!(machine.reclaim(false), vec![abandoned.clone(), abandoned_file.clone()]);
    assert!(!abandoned.exists() && !abandoned_file.exists());
    assert!(fresh.exists() && working.exists() && held.exists() && display.exists() && multiplexer.exists());
}

#[test]
fn a_dry_run_names_what_it_would_remove_and_removes_nothing() {
    let machine = Machine::new();
    let abandoned = machine.dir("tmp/sc420", 3 * DAY);

    assert_eq!(machine.reclaim(true), vec![abandoned.clone()]);
    assert!(abandoned.exists());
}

#[test]
fn lan_pair_copies_go_only_when_idle_unqueued_unlocked_and_not_a_default_pair() {
    let machine = Machine::new();
    let pair = |number: u32, age: Duration| {
        let clients = ["a", "b"].map(|side| machine.dir(&format!("clients/lan{number}{side}"), 3 * DAY));
        machine.dir(&format!("state/pair-{number}"), age);
        clients
    };
    let default = pair(1, 3 * DAY);
    let idle = pair(5, 3 * DAY);
    let queued = pair(6, 3 * DAY);
    machine.dir("state/pair-6/queue", 3 * DAY);
    machine.file("state/pair-6/queue/sc-363", 3 * DAY);
    touch(&machine.path("state/pair-6/queue"), machine.now - 3 * DAY);
    touch(&machine.path("state/pair-6"), machine.now - 3 * DAY);
    let recent = pair(7, Duration::from_secs(1800));
    let locked = pair(8, 3 * DAY);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(machine.path("state/pair-8/pool.lock")).unwrap();
    touch(&machine.path("state/pair-8/pool.lock"), machine.now - 3 * DAY);
    touch(&machine.path("state/pair-8"), machine.now - 3 * DAY);
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();

    assert_eq!(machine.reclaim(false), idle.to_vec());
    assert!(idle.iter().all(|client| !client.exists()));
    assert!([default, queued, recent, locked].iter().flatten().all(|client| client.exists()));
}

#[test]
fn lan_pair_copies_a_pool_json_names_stay_however_long_the_pair_sat_idle() {
    let machine = Machine::new();
    let pair = |number: u32| {
        let clients = ["a", "b"].map(|side| machine.dir(&format!("clients/lan{number}{side}"), 3 * DAY));
        machine.dir(&format!("state/pair-{number}"), 3 * DAY);
        clients
    };
    let pooled = [pair(4), pair(5)];
    let unlisted = pair(9);
    fs::write(machine.path("state/pool.json"), r#"{"profile":"parity","pairs":[{"id":4,"clients":"x"},{"id":5,"clients":"x"}]}"#).unwrap();

    assert_eq!(machine.reclaim(false), unlisted.to_vec());
    assert!(pooled.iter().flatten().all(|client| client.exists()));
}

fn cargo_target(machine: &Machine, relative: &str, age: Duration) -> PathBuf {
    let target = machine.dir(relative, age);
    let tag = machine.file(&format!("{relative}/CACHEDIR.TAG"), age);
    touch(&tag, machine.now - age);
    touch(&target, machine.now - age);
    target
}

#[test]
fn idle_cargo_targets_go_from_north_main_and_every_containers_worktrees_but_not_other_mains_or_node_modules() {
    let machine = Machine::new();
    let main = cargo_target(&machine, "code/north/main/target", 3 * DAY);
    let lane = cargo_target(&machine, "code/smashcraft/worktrees/sc-1/target", 3 * DAY);
    let wisp_lane = cargo_target(&machine, "code/wisp/worktrees/w1/target", 3 * DAY);
    let installed = machine.dir("code/smashcraft/worktrees/sc-1/node_modules", 3 * DAY);
    let smashcraft_main = cargo_target(&machine, "code/smashcraft/main/target", 3 * DAY);
    let wisp_main = cargo_target(&machine, "code/wisp/main/target", 3 * DAY);
    let north_modules = machine.dir("code/north/main/node_modules", 3 * DAY);
    let busy = cargo_target(&machine, "code/north/worktrees/busy/target", 3 * DAY);
    machine.file("code/north/worktrees/busy/target/debug.log", Duration::from_secs(60));
    let held = cargo_target(&machine, "code/north/worktrees/held/target", 3 * DAY);
    machine.dir("proc/4242", DAY);
    symlink(held.join("deps"), machine.path("proc/4242/cwd")).unwrap();
    let untagged = machine.dir("code/north/worktrees/plain/target", 3 * DAY);
    let other = cargo_target(&machine, "code/casclib/worktrees/c1/target", 3 * DAY);
    let hours_old = cargo_target(&machine, "code/north/worktrees/hours/target", Duration::from_secs(5 * 3600));
    let recent = cargo_target(&machine, "code/north/worktrees/recent/target", Duration::from_secs(3600));
    let source = machine.file("code/smashcraft/worktrees/sc-1/src.rs", 3 * DAY);

    assert_eq!(machine.reclaim(false), vec![other.clone(), main.clone(), hours_old.clone(), lane.clone(), wisp_lane.clone()]);
    assert!(!main.exists() && !lane.exists() && !wisp_lane.exists() && !other.exists());
    assert!(!hours_old.exists());
    assert!(installed.exists() && smashcraft_main.exists() && wisp_main.exists() && north_modules.exists());
    assert!(busy.exists() && held.exists() && recent.exists() && untagged.exists() && source.exists());
}

#[test]
fn a_lane_private_cargo_target_in_the_cache_goes_idle_but_the_shared_one_and_a_held_one_stay() {
    let machine = Machine::new();
    let private = cargo_target(&machine, "cache/cargo-target-lane7", 3 * DAY);
    let shared = cargo_target(&machine, "cache/cargo-target", 3 * DAY);
    let fresh = cargo_target(&machine, "cache/cargo-target-fresh", Duration::from_secs(600));
    let held = cargo_target(&machine, "cache/cargo-target-held", 3 * DAY);
    machine.dir("proc/4242/fd", DAY);
    symlink(held.join("lock"), machine.path("proc/4242/fd/3")).unwrap();

    assert_eq!(machine.reclaim(false), vec![private.clone()]);
    assert!(!private.exists() && shared.exists() && fresh.exists() && held.exists());
}

#[test]
fn reclaim_trims_a_shared_cargo_target_over_its_cap_and_a_dry_run_leaves_it() {
    let machine = Machine::new();
    let deps = machine.dir("cache/cargo-target/debug/deps", 3 * DAY);
    let binary = deps.join("north-1a2b3c");
    let file = File::create(&binary).unwrap();
    file.set_len(11 << 30).unwrap();
    fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    touch(&binary, machine.now - 3 * DAY);

    assert!(machine.reclaim(true).is_empty());
    assert!(binary.exists());
    let removals = reclaim(&machine.inputs(), &Policy { idle: DAY, dry_run: false }).unwrap();
    assert_eq!(removals.iter().map(|removal| &removal.path).collect::<Vec<_>>(), vec![&machine.path("cache/cargo-target")]);
    assert_eq!(summary(&removals, false), "removed 1 entries; 11264 MiB freed");
    assert!(!binary.exists());
}

fn cache_job(machine: &Machine, relative: &str, age: Duration, gib: u64) -> PathBuf {
    let blob = machine.file(&format!("{relative}/source/blob"), age);
    File::options().write(true).open(&blob).unwrap().set_len(gib << 30).unwrap();
    touch(&blob, machine.now - age);
    machine.path(relative)
}

#[test]
fn a_build_cache_over_its_cap_loses_its_oldest_idle_unheld_children_until_it_fits() {
    let machine = Machine::new();
    let oldest = cache_job(&machine, "cache/nexus-run/aaa", 5 * DAY, 1);
    let held = cache_job(&machine, "cache/nexus-run/bbb", 4 * DAY, 1);
    let older = cache_job(&machine, "cache/nexus-run/ccc", 3 * DAY, 1);
    let newer = cache_job(&machine, "cache/nexus-run/ddd", Duration::from_secs(7200), 1);
    let fresh = cache_job(&machine, "cache/nexus-run/eee", Duration::from_secs(600), 1);
    machine.dir("proc/4242/fd", DAY);
    symlink(held.join("source/blob"), machine.path("proc/4242/fd/3")).unwrap();

    assert_eq!(machine.reclaim(true), vec![oldest.clone(), older.clone()]);
    assert!(oldest.exists() && older.exists());
    assert_eq!(machine.reclaim(false), vec![oldest.clone(), older.clone()]);
    assert!(!oldest.exists() && !older.exists() && held.exists() && newer.exists() && fresh.exists());
}

#[test]
fn a_build_cache_within_its_cap_keeps_every_child() {
    let machine = Machine::new();
    let old = cache_job(&machine, "cache/nexus-run/aaa", 9 * DAY, 1);
    let target = cache_job(&machine, "cache/galileo-full-suite/target/debug", 9 * DAY, 3);

    assert!(machine.reclaim(false).is_empty());
    assert!(old.exists() && target.exists());
}

fn budgeted(machine: &Machine, policy: &str) -> Inputs {
    Inputs { disk_budget: DiskBudget::parse(policy, "gex45"), ..machine.inputs() }
}

#[test]
fn the_shared_cargo_target_sweep_reads_its_cap_from_the_hosts_disk_budget() {
    let machine = Machine::new();
    let deps = machine.dir("cache/cargo-target/debug/deps", 3 * DAY);
    let binary = deps.join("north-1a2b3c");
    fs::write(&binary, vec![0u8; 3 << 20]).unwrap();
    fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    touch(&binary, machine.now - 3 * DAY);
    let policy = Policy { idle: DAY, dry_run: false };

    let roomy = budgeted(&machine, "[hosts.gex45.disk_budget_gib]\ncargo_target = 1\n");
    assert!(reclaim(&roomy, &policy).unwrap().is_empty() && binary.exists());
    let tight = budgeted(&machine, "[hosts.gex45.disk_budget_gib]\ncargo_target = 0.001\n");
    assert_eq!(reclaim(&tight, &policy).unwrap().len(), 1);
    assert!(!binary.exists());
}

#[test]
fn the_evidence_sweep_reads_its_budget_from_the_hosts_disk_budget() {
    let machine = Machine::new();
    let run = machine.dir("evidence/old-run", 9 * DAY);
    fs::write(run.join("frame.png"), vec![7u8; 2 << 20]).unwrap();
    touch(&run, machine.now - 9 * DAY);
    touch(&run.join("frame.png"), machine.now - 9 * DAY);
    let policy = Policy { idle: DAY, dry_run: false };

    let roomy = budgeted(&machine, "[hosts.gex45.disk_budget_gib]\nsmashcraft_evidence = 1\n");
    assert!(reclaim(&roomy, &policy).unwrap().is_empty() && run.exists());
    let tight = budgeted(&machine, "[hosts.gex45.disk_budget_gib]\nsmashcraft_evidence = 0.001\n");
    assert_eq!(reclaim(&tight, &policy).unwrap().len(), 1);
    assert!(!run.exists());
}
