use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, flock};

use super::{Inputs, Policy, reclaim};

const DAY: Duration = Duration::from_secs(24 * 3600);

struct Machine {
    root: tempfile::TempDir,
    now: SystemTime,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["tmp", "proc", "clients", "state"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        Self { root, now: SystemTime::now() + 2 * DAY }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn dir(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).unwrap();
        touch(&path, self.now - age);
        path
    }

    fn file(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::write(&path, b"x").unwrap();
        touch(&path, self.now - age);
        path
    }

    fn inputs(&self) -> Inputs {
        Inputs {
            tmp: self.path("tmp"),
            proc_root: self.path("proc"),
            lan_clients: self.path("clients"),
            lan_state: self.path("state"),
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

fn touch(path: &Path, at: SystemTime) {
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
