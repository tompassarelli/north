use std::fs;
use std::time::{Duration, SystemTime};

use super::scratch::{drop_scratch_target, scratch_targets};
use super::targets::BuildTarget;

#[test]
fn an_idle_unheld_cargo_target_in_scratch_space_is_dropped_and_nothing_else_is() {
    let scratch = tempfile::tempdir().unwrap();
    let scratch = scratch.path();
    let tag = "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n";
    for target in [scratch.join("clippy-target"), scratch.join("checkout/target")] {
        fs::create_dir_all(target.join("debug/deps")).unwrap();
        fs::write(target.join("CACHEDIR.TAG"), tag).unwrap();
        fs::write(target.join("debug/deps/out"), vec![7u8; 64 * 1024]).unwrap();
    }
    fs::create_dir_all(scratch.join("notes")).unwrap();
    fs::write(scratch.join("notes/CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();

    let targets = scratch_targets(scratch);
    assert_eq!(targets, vec![scratch.join("checkout/target"), scratch.join("clippy-target")], "only directories cargo tagged as its target");

    let idle = Duration::from_secs(3 * 3600);
    let later = SystemTime::now() + 2 * idle;
    let target = &targets[1];
    assert_eq!(drop_scratch_target(target, &[], SystemTime::now(), idle, false), BuildTarget::InUse, "a target built within the idle window stays");
    assert_eq!(drop_scratch_target(target, &[target.join("debug/.cargo-lock")], later, idle, false), BuildTarget::InUse, "a target a process holds open stays");
    assert!(matches!(drop_scratch_target(target, &[], later, idle, true), BuildTarget::Dropped(bytes) if bytes >= 64 * 1024));
    assert!(target.exists(), "a dry run removes nothing");
    assert!(matches!(drop_scratch_target(target, &[], later, idle, false), BuildTarget::Dropped(bytes) if bytes >= 64 * 1024));
    assert!(!target.exists());
    assert!(scratch.join("checkout").exists() && scratch.join("notes").exists());
}
