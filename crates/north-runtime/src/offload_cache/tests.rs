use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, flock};

use super::*;

const KIB: usize = 1 << 10;

fn slot(root: &Path, name: &str, kib: usize, age_hours: u64) {
    fs::create_dir_all(root.join(name).join("target")).unwrap();
    fs::write(root.join(name).join("target/blob"), vec![7u8; kib * KIB]).unwrap();
    let lock = File::create(root.join(format!("{name}.lock"))).unwrap();
    lock.set_modified(SystemTime::now() - Duration::from_secs(age_hours * 3600)).unwrap();
}

fn hold(root: &Path, name: &str) -> File {
    let lock = File::open(root.join(format!("{name}.lock"))).unwrap();
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
    lock
}

fn names(evicted: &[Evicted]) -> Vec<String> {
    evicted.iter().map(|gone| gone.slot.file_name().unwrap().to_string_lossy().into_owned()).collect()
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
    let evicted = evict(&[Store { root: root.clone(), cap_bytes: Some(cap) }], EVICT_ABOVE_PERCENT, &mut || None);
    assert_eq!(names(&evicted), ["older"]);
    assert!(!root.join("older").exists() && !root.join("older.bundle").exists());
    assert!(root.join("older.lock").exists(), "the lock file stays, so a job opening it meanwhile still serializes");
    assert!(root.join("oldest-locked/target/blob").exists() && root.join("newest/target/blob").exists());
}

#[test]
fn disk_floor_reclaim_evicts_offload_slots_oldest_first_until_the_disk_is_under_the_line() {
    let home = tempfile::tempdir().unwrap();
    let build = home.path().join(".cache/build-offload");
    let land = home.path().join(".cache/land-offload");
    slot(&build, "lane-a", 8, 9);
    slot(&build, "lane-b", 8, 5);
    slot(&land, "code-north-main", 8, 40);
    fs::create_dir_all(land.join("code-north-main.beside/south")).unwrap();
    let mut readings = [95.0, 95.0, 90.0, 82.0, 79.0].into_iter();
    let freed = reclaim_in(home.path(), &mut || readings.next().map(|used_percent| Disk { used_percent, free_gib: used_percent, size_bytes: 1 << 40 }), || None);
    assert_eq!(freed, Some(79.0), "the reading after eviction, though the other sweeps were on cooldown");
    assert!(!build.join("lane-a").exists() && !build.join("lane-b").exists() && !land.join("code-north-main").exists());
    assert!(!land.join("code-north-main.beside").exists());
    let empty = tempfile::tempdir().unwrap();
    let mut calm = || Some(Disk { used_percent: 50.0, free_gib: 400.0, size_bytes: 1 << 40 });
    assert_eq!(reclaim_in(empty.path(), &mut calm, || None), None, "nothing evicted and no other sweep ran, so admission keeps its reading");
    assert_eq!(reclaim_in(empty.path(), &mut calm, || Some(12.5)), Some(12.5));
}

#[test]
fn evicting_a_worktree_slot_unregisters_it_and_its_beside_checkouts_so_the_next_job_can_add_them_again() {
    let home = tempfile::tempdir().unwrap();
    let repository = |name: &str| {
        let repo = home.path().join("code").join(name).join("main");
        fs::create_dir_all(&repo).unwrap();
        let run = |arguments: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).status().unwrap().success());
        run(&["init", "-q"]);
        run(&["commit", "-q", "--allow-empty", "-m", "first"]);
        repo
    };
    let (north, south) = (repository("north"), repository("south"));
    let root = home.path().join(".cache/land-offload");
    let (slot, beside) = (root.join("code-north-main-4"), root.join("code-north-main-4.beside/south"));
    let add = |repo: &Path, at: &Path| std::process::Command::new("git").arg("-C").arg(repo).args(["worktree", "add", "-q", "--detach"]).arg(at).status().unwrap().success();
    assert!(add(&north, &slot) && add(&south, &beside));
    File::create(root.join("code-north-main-4.lock")).unwrap();
    let evicted = evict(&[Store { root, cap_bytes: Some(0) }], EVICT_ABOVE_PERCENT, &mut || None);
    assert_eq!(names(&evicted), ["code-north-main-4"]);
    assert!(fs::read_dir(north.join(".git/worktrees")).unwrap().next().is_none() && fs::read_dir(south.join(".git/worktrees")).unwrap().next().is_none());
    assert!(add(&north, &slot) && add(&south, &beside), "git refuses a missing but already registered worktree");
}
