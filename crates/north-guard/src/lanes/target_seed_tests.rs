use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::seed_or_build_cold;

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
