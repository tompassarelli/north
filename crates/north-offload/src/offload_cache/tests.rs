use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, flock};

use super::*;

const KIB: usize = 1 << 10;

/// The clock every fixture and the eviction pass share, so a slot's age orders the eviction without reading this host's wall clock.
fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600)
}

fn slot(root: &Path, name: &str, kib: usize, age_hours: u64) {
    fs::create_dir_all(root.join(name).join("target")).unwrap();
    fs::write(root.join(name).join("target/blob"), vec![7u8; kib * KIB]).unwrap();
    let lock = File::create(root.join(format!("{name}.lock"))).unwrap();
    lock.set_modified(now() - Duration::from_secs(age_hours * 3600)).unwrap();
}

fn hold(root: &Path, name: &str) -> File {
    let lock = File::open(root.join(format!("{name}.lock"))).unwrap();
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
    lock
}

fn names(evicted: &[Evicted]) -> Vec<String> {
    evicted.iter().map(|gone| gone.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn build_offload_over_its_cap_evicts_the_least_recently_used_unlocked_slot_and_keeps_a_locked_one() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".cache/build-offload");
    slot(&root, "oldest-locked", 64, 30);
    slot(&root, "older", 64, 20);
    slot(&root, "newest", 64, 1);
    fs::write(root.join("older.bundle"), b"stale").unwrap();
    let _held = hold(&root, "oldest-locked");
    let cap = allocated_tree(&root.join("newest"), &mut HashSet::new()).unwrap().0 * 2 + 1;
    let evicted = evict(&[Store::Slots { root: root.clone(), cap_bytes: Some(cap), take: Take::Every, line: Line::NEVER }], now(), &mut || None);
    assert_eq!(names(&evicted), ["older"]);
    assert!(!root.join("older").exists() && !root.join("older.bundle").exists());
    assert!(root.join("older.lock").exists(), "the lock file stays, so a job opening it meanwhile still serializes");
    assert!(root.join("oldest-locked/target/blob").exists() && root.join("newest/target/blob").exists());
}

#[test]
fn own_stores_prune_keeps_the_pinned_the_leased_and_the_running_runtime_and_deletes_the_rest() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("north");
    let runtimes = state.join("runtimes");
    let sha = |index: usize| format!("{index:040x}");
    for index in 0..16 {
        let runtime = runtimes.join(sha(index));
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("north"), b"").unwrap();
        fs::write(runtime.join("lease"), b"").unwrap();
        File::open(&runtime).unwrap().set_modified(now() - Duration::from_secs(3600 * (100 - index as u64))).unwrap();
    }
    let leased = File::open(runtimes.join(sha(1)).join("lease")).unwrap();
    flock(&leased, FlockOperation::NonBlockingLockExclusive).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{}", sha(0)), state.join("current")).unwrap();
    let processes = home.path().join("proc");
    fs::create_dir_all(processes.join("7")).unwrap();
    fs::write(processes.join("7/cmdline"), format!("{}/north\0", runtimes.join(sha(2)).display())).unwrap();
    let queue = state.join("land-queue/abc/logs");
    fs::create_dir_all(&queue).unwrap();
    File::create(queue.join("old.log")).unwrap().set_modified(now() - Duration::from_secs(8 * 86_400)).unwrap();
    File::create(queue.join("new.log")).unwrap().set_modified(now() - Duration::from_secs(86_400)).unwrap();

    own_stores::prune(home.path(), &processes, now(), true);

    for survivor in [0, 1, 2, 6, 15] {
        assert!(runtimes.join(sha(survivor)).is_dir(), "runtime {survivor} was deleted");
    }
    for gone in [3, 4, 5] {
        assert!(!runtimes.join(sha(gone)).exists(), "runtime {gone} survived");
    }
    assert!(!queue.join("old.log").exists() && queue.join("new.log").exists());
}
