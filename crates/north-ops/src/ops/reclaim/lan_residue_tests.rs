use std::fs::{self, OpenOptions};
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::Duration;

use rustix::fs::{FlockOperation, flock};

use super::tests::{DAY, Machine, touch};
use super::{Policy, reclaim};

const DOCUMENTS: &str = "pfx/drive_c/users/steamuser/Documents/Warcraft III";

fn client(machine: &Machine, name: &str) -> [PathBuf; 4] {
    let file = |relative: &str, age: Duration| {
        let path = machine.path(&format!("clients/{name}/{DOCUMENTS}/{relative}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![7u8; 64 << 10]).unwrap();
        touch(&path, machine.now - age);
        path
    };
    [file("Maps/Wisp/old.w3x", 3 * DAY), file("CustomMapData/run/p1.txt", 3 * DAY), file("Maps/Wisp/today.w3x", Duration::from_secs(600)), file("Preferences/War3Preferences.txt", 3 * DAY)]
}

fn pair(machine: &Machine, number: u32) -> [[PathBuf; 4]; 2] {
    machine.dir(&format!("state/pair-{number}"), 3 * DAY);
    ["a", "b"].map(|side| client(machine, &format!("lan{number}{side}")))
}

fn sweep(machine: &Machine, dry_run: bool) -> usize {
    reclaim(&machine.inputs(), &Policy { idle: DAY, dry_run }).unwrap().len()
}

#[test]
fn stale_map_copies_and_custom_map_data_go_from_every_client_including_pool_pairs_but_fresh_files_and_other_files_stay() {
    let machine = Machine::new();
    let pooled = pair(&machine, 4);
    fs::write(machine.path("state/pool.json"), r#"{"pairs":[{"id":4}]}"#).unwrap();
    machine.dir("state/pair-4/queue", 3 * DAY);
    fs::write(machine.path("state/pair-4/queue/next"), b"13").unwrap();
    fs::write(machine.path("state/pair-4/queue/next.lock"), b"").unwrap();

    assert_eq!(sweep(&machine, true), 4);
    assert!(pooled.iter().flatten().all(|file| file.exists()));
    assert_eq!(sweep(&machine, false), 4);
    for client in &pooled {
        assert!(!client[0].exists() && !client[1].exists());
        assert!(client[2].exists() && client[3].exists());
    }
}

#[test]
fn a_queued_locked_or_held_pair_keeps_its_map_copies() {
    let machine = Machine::new();
    let queued = pair(&machine, 6);
    machine.dir("state/pair-6/queue", 3 * DAY);
    fs::write(machine.path("state/pair-6/queue/000000000007.json"), b"x").unwrap();
    let locked = pair(&machine, 8);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(machine.path("state/pair-8/pool.lock")).unwrap();
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
    let held = pair(&machine, 9);
    machine.dir("proc/4242/fd", DAY);
    symlink(&held[0][0], machine.path("proc/4242/fd/3")).unwrap();

    assert_eq!(sweep(&machine, false), 0);
    assert!([queued, locked, held].iter().flatten().flatten().all(|file| file.exists()));
}

#[test]
fn a_signed_in_client_loses_stale_map_copies_unless_a_process_holds_it() {
    let machine = Machine::new();
    let idle = client(&machine, "../online/clone-b");
    let held = client(&machine, "../online/clone-c");
    machine.dir("proc/4242/fd", DAY);
    symlink(fs::canonicalize(&held[0]).unwrap(), machine.path("proc/4242/fd/3")).unwrap();

    assert_eq!(sweep(&machine, false), 2);
    assert!(!idle[0].exists() && !idle[1].exists());
    assert!(idle[2].exists() && idle[3].exists());
    assert!(held.iter().all(|file| file.exists()));
}
