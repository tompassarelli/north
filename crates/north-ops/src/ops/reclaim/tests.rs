use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};


use super::{Inputs, Policy, reclaim};
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
            online_clients: self.path("online"),
            lan_state: self.path("state"),
            evidence: self.path("evidence"),
            capture_queue: self.path("capture-queue"),
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

