use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{reclaim_dead_seeds, seed_or_build_cold};

fn checkout(dir: &Path) {
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    fs::write(dir.join("Cargo.lock"), "version = 4\n").unwrap();
}

#[test]
fn a_sibling_target_that_cannot_be_copied_leaves_the_lane_to_build_cold_instead_of_failing_its_check() {
    let container = tempfile::tempdir().unwrap();
    let main = container.path().join("main");
    let lane = container.path().join("worktrees/lander-replay");
    checkout(&main);
    checkout(&lane);
    let target = main.join("target");
    fs::create_dir_all(target.join("debug/deps")).unwrap();
    fs::write(target.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
    let vanishing = target.join("debug/deps/libnorth_fleet.rlib");
    fs::write(&vanishing, "rlib").unwrap();
    fs::set_permissions(&vanishing, fs::Permissions::from_mode(0o000)).unwrap();
    let status = seed_or_build_cold(&lane);
    fs::set_permissions(&vanishing, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(status, 0, "a failed seed copy must not fail the land check that asked for it");
    assert!(!lane.join("target").exists(), "a failed seed leaves no partial target behind");
    assert!(fs::read_dir(&lane).unwrap().flatten().all(|entry| !entry.file_name().to_string_lossy().starts_with("target.seeding-")));
}

fn seed_dir(parent: &Path, pid: u32, age: Duration, now: SystemTime) -> PathBuf {
    let path = parent.join(format!("target.seeding-{pid}"));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
    fs::File::open(&path).unwrap().set_modified(now - age).unwrap();
    path
}

#[test]
fn a_dead_seeds_partial_target_is_reclaimed() {
    let root = tempfile::tempdir().unwrap();
    let code = root.path().join("code");
    let proc_root = root.path().join("proc");
    let main = code.join("north/main");
    let lane = code.join("north/worktrees/lander");
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(&lane).unwrap();
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let dead_main = seed_dir(&main, 3_999_999, Duration::from_secs(2 * 3600), now);
    let dead_lane = seed_dir(&lane, 4_000_001, Duration::from_secs(2 * 3600), now);
    let alive = seed_dir(&main, 4_000_002, Duration::from_secs(2 * 3600), now);
    fs::create_dir_all(proc_root.join("4000002")).unwrap();
    let fresh = seed_dir(&main, 4_000_003, Duration::from_secs(60), now);

    let removed: Vec<PathBuf> = reclaim_dead_seeds(&code, &proc_root, now, false).into_iter().map(|seed| seed.path).collect();

    assert_eq!(removed, vec![dead_main.clone(), dead_lane.clone()]);
    assert!(!dead_main.exists() && !dead_lane.exists());
    assert!(alive.exists(), "a seeding process that is still alive keeps its copy");
    assert!(fresh.exists(), "a dead seed younger than an hour may still be a live copy of a just-killed process");
}
