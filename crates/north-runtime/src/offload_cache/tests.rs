use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, flock};

use super::*;

const KIB: usize = 1 << 10;
const NEVER: Line = Line { free_share: 0.0, free_gib: 0.0 };

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
    let evicted = evict(&[Store::Slots { root: root.clone(), cap_bytes: Some(cap), take: Take::Every }], NEVER, &mut || None);
    assert_eq!(names(&evicted), ["older"]);
    assert!(!root.join("older").exists() && !root.join("older.bundle").exists());
    assert!(root.join("older.lock").exists(), "the lock file stays, so a job opening it meanwhile still serializes");
    assert!(root.join("oldest-locked/target/blob").exists() && root.join("newest/target/blob").exists());
}

#[test]
fn disk_floor_reclaim_takes_build_slots_then_shared_build_output_and_sccache_to_their_caps_then_land_slots_the_landers_last() {
    let home = tempfile::tempdir().unwrap();
    let build = home.path().join(".cache/build-offload");
    let land = home.path().join(".cache/land-offload");
    slot(&build, "lane-a", 8, 9);
    slot(&build, "lane-b", 8, 5);
    slot(&land, "code-north-main", 8, 40);
    fs::create_dir_all(land.join("code-north-main.beside/south")).unwrap();
    let mut readings = [5.0, 5.0, 10.0, 10.0, 10.0, 18.0, 100.0].into_iter();
    let freed = reclaim_in(home.path(), &mut || readings.next().map(|free_gib| Disk { used_percent: 90.0, free_gib, size_bytes: 1 << 40 }), || None);
    assert_eq!(freed, Some(100.0), "the reading after eviction, though the other sweeps were on cooldown");
    assert!(!build.join("lane-a").exists() && !build.join("lane-b").exists() && !land.join("code-north-main").exists());
    assert!(!land.join("code-north-main.beside").exists());
    let empty = tempfile::tempdir().unwrap();
    let mut calm = || Some(Disk { used_percent: 50.0, free_gib: 400.0, size_bytes: 1 << 40 });
    assert_eq!(reclaim_in(empty.path(), &mut calm, || None), None, "nothing evicted and no other sweep ran, so admission keeps its reading");
    assert_eq!(reclaim_in(empty.path(), &mut calm, || Some(12.5)), Some(12.5));
    let swept_first = tempfile::tempdir().unwrap();
    slot(&swept_first.path().join(".cache/land-offload"), "code-north-main", 8, 40);
    static SWEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let sweep = || {
        SWEPT.store(true, std::sync::atomic::Ordering::SeqCst);
        Some(400.0)
    };
    let mut full_until_swept = || Some(Disk { used_percent: 90.0, free_gib: if SWEPT.load(std::sync::atomic::Ordering::SeqCst) { 400.0 } else { 5.0 }, size_bytes: 1 << 40 });
    assert_eq!(reclaim_in(swept_first.path(), &mut full_until_swept, sweep), Some(400.0));
    assert!(swept_first.path().join(".cache/land-offload/code-north-main/target/blob").exists(), "the other sweeps run first, so a disk they bring under the line keeps its land slots");
    shared_build_output_and_sccache_go_to_their_caps_before_any_land_slot_and_the_landers_slot_goes_last();
    a_large_disk_evicts_at_the_same_free_headroom_as_a_small_one_so_a_roomy_host_keeps_its_land_slots();
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
    let evicted = evict(&[Store::Slots { root, cap_bytes: Some(0), take: Take::Every }], NEVER, &mut || None);
    assert_eq!(names(&evicted), ["code-north-main-4"]);
    assert!(fs::read_dir(north.join(".git/worktrees")).unwrap().next().is_none() && fs::read_dir(south.join(".git/worktrees")).unwrap().next().is_none());
    assert!(add(&north, &slot) && add(&south, &beside), "git refuses a missing but already registered worktree");
}

fn aged(path: &Path, kib: usize, age_hours: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7u8; kib * KIB]).unwrap();
    File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(age_hours * 3600)).unwrap();
}

fn shared_build_output_and_sccache_go_to_their_caps_before_any_land_slot_and_the_landers_slot_goes_last() {
    let lay_out = || {
        let home = tempfile::tempdir().unwrap();
        aged(&home.path().join(".cache/cargo-target/north/debug/incremental/north_core-1/blob"), 512, 3);
        aged(&home.path().join(".cache/cargo-target/wisp-window/debug/incremental/wisp-1/blob"), 512, 1);
        aged(&home.path().join("sccache-store/a/b/older"), 256, 5);
        aged(&home.path().join("sccache-store/a/c/newer"), 256, 1);
        let land = home.path().join(".cache/land-offload");
        slot(&land, "code-north-main-5", 64, 50);
        slot(&land, "code-north-main", 64, 40);
        slot(&land, "code-north-main-2", 64, 10);
        let budget = DiskBudget::parse("[hosts.h.disk_budget_gib]\ncargo_target = 0.00048828125\nsccache = 0.000244140625\n", "h");
        let stores: Vec<Store> = stores(home.path(), None, &budget)
            .into_iter()
            .map(|store| match store {
                Store::CompilerCache { cap_bytes, .. } => Store::CompilerCache { root: home.path().join("sccache-store"), cap_bytes },
                other => other,
            })
            .collect();
        (home, stores)
    };
    let used_kib = |home: &Path| allocated_tree(home, &mut HashSet::new()).unwrap().0 as f64 / KIB as f64;
    let (home, stores) = lay_out();
    let room = 1e9;
    let line = Line { free_share: 1.0, free_gib: room - (used_kib(home.path()) - 600.0) };
    let evicted = evict(&stores, line, &mut || Some(Disk { used_percent: 0.0, free_gib: room - used_kib(home.path()), size_bytes: u64::MAX }));
    assert_eq!(evicted.iter().map(|gone| gone.what).collect::<Vec<_>>(), ["idle build output of shared cargo target", "oldest entries of compiler cache"]);
    assert!(!home.path().join("sccache-store/a/b/older").exists() && home.path().join("sccache-store/a/c/newer").exists(), "trimmed to its cap, oldest entry first");
    assert!(home.path().join(".cache/cargo-target/wisp-window/debug/incremental/wisp-1/blob").exists(), "the shared targets are one store held to the cap, least recently built first");
    assert!(["code-north-main-5", "code-north-main", "code-north-main-2"].iter().all(|name| home.path().join(".cache/land-offload").join(name).join("target/blob").exists()));
    let (_home, stores) = lay_out();
    let always = Line { free_share: 1.0, free_gib: f64::INFINITY };
    let evicted = evict(&stores, always, &mut || Some(Disk { used_percent: 100.0, free_gib: 0.0, size_bytes: 1 << 40 }));
    assert_eq!(names(&evicted), ["north", "sccache-store", "code-north-main", "code-north-main-2", "code-north-main-5"], "the lander's reserved slot goes last though it was used least recently");
}

fn a_large_disk_evicts_at_the_same_free_headroom_as_a_small_one_so_a_roomy_host_keeps_its_land_slots() {
    let disk = |size_gib: f64, used_percent: f64| Disk { used_percent, free_gib: size_gib * (100.0 - used_percent) / 100.0, size_bytes: (size_gib * GIB) as u64 };
    let line = Line::eviction();
    assert!(!line.passed_by(disk(936.0, 81.0)), "gex45's 936 GiB disk at 81% still has 178 GiB free");
    assert!(line.passed_by(disk(936.0, 91.0)));
    assert!(line.passed_by(disk(224.0, 81.0)) && !line.passed_by(disk(224.0, 79.0)), "ccx33's 224 GiB disk keeps the 20% line");
    assert!(!Line::incident().passed_by(disk(936.0, 86.0)) && Line::incident().passed_by(disk(224.0, 86.0)));
    let home = tempfile::tempdir().unwrap();
    let land = home.path().join(".cache/land-offload");
    slot(&land, "code-north-main-3", 8, 40);
    slot(&land, "code-north-main", 8, 50);
    let evicted = evict(&stores(home.path(), Some(disk(936.0, 81.0)), &DiskBudget::default()), line, &mut || Some(disk(936.0, 81.0)));
    assert!(evicted.is_empty() && land.join("code-north-main-3/target/blob").exists() && land.join("code-north-main/target/blob").exists());
}
