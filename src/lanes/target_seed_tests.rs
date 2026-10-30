use std::fs::{self, File};
use std::path::Path;

use super::target_seed::seed;

const LOCK: &str = "[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

fn checkout(root: &Path, lock: &str) {
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join(".git"), "gitdir: elsewhere").unwrap();
    fs::write(root.join("sub/Cargo.toml"), "[package]\nname = \"app\"\nversion = \"0.0.0\"\n").unwrap();
    fs::write(root.join("sub/Cargo.lock"), lock).unwrap();
}

fn build(root: &Path) {
    for profile in ["sub/target/debug", "sub/target/x86_64-unknown-linux-gnu/debug"] {
        let debug = root.join(profile);
        fs::create_dir_all(debug.join(".fingerprint/serde-0123456789abcdef")).unwrap();
        fs::create_dir_all(debug.join(".fingerprint/app-0123456789abcdef")).unwrap();
        fs::write(debug.join("libserde.rlib"), "built").unwrap();
    }
    fs::write(root.join("sub/target/CACHEDIR.TAG"), "").unwrap();
}

fn lanes(scratch: &Path, lane_lock: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let (main, lane) = (scratch.join("main"), scratch.join("worktrees/lane"));
    checkout(&main, LOCK);
    build(&main);
    checkout(&lane, lane_lock);
    (main, lane)
}

#[test]
fn a_new_lane_copies_a_sibling_target_built_from_the_same_lockfile_and_rebuilds_only_its_own_packages() {
    let scratch = tempfile::tempdir().unwrap();
    let (main, lane) = lanes(scratch.path(), LOCK);
    assert_eq!(seed(&lane.join("sub")).unwrap(), Some(main.join("sub/target")));
    for profile in ["debug", "x86_64-unknown-linux-gnu/debug"] {
        let debug = lane.join("sub/target").join(profile);
        assert_eq!(fs::read_to_string(debug.join("libserde.rlib")).unwrap(), "built");
        assert!(debug.join(".fingerprint/serde-0123456789abcdef").is_dir());
        assert!(!debug.join(".fingerprint/app-0123456789abcdef").exists());
    }
    assert!(main.join("sub/target/debug/.fingerprint/app-0123456789abcdef").is_dir());
}

#[test]
fn a_lane_with_a_target_or_another_lockfile_is_left_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let (_, lane) = lanes(scratch.path(), &format!("{LOCK}\n"));
    assert_eq!(seed(&lane.join("sub")).unwrap(), None);
    assert!(!lane.join("sub/target").exists());
    fs::write(lane.join("sub/Cargo.lock"), LOCK).unwrap();
    fs::create_dir_all(lane.join("sub/target")).unwrap();
    assert_eq!(seed(&lane.join("sub")).unwrap(), None);
}

#[test]
fn a_sibling_that_is_mid_build_is_not_copied() {
    let scratch = tempfile::tempdir().unwrap();
    let (main, lane) = lanes(scratch.path(), LOCK);
    let building = File::create(main.join("sub/target/debug/.cargo-lock")).unwrap();
    building.lock().unwrap();
    assert_eq!(seed(&lane.join("sub")).unwrap(), None);
    drop(building);
    assert_eq!(seed(&lane.join("sub")).unwrap(), Some(main.join("sub/target")));
}
