use std::collections::HashSet;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;

use super::*;

const HOUR: Duration = Duration::from_secs(3600);

fn write(path: &Path, bytes: usize, mode: u32, age: Duration) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![0u8; bytes]).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - age).unwrap();
}

fn sweep_now(target: &Path, cap: u64) -> u64 {
    sweep(target, cap, IDLE, SystemTime::now()).unwrap()
}

#[test]
fn an_idle_executable_and_an_idle_incremental_directory_go_but_libraries_stay() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("debug/deps/north-1"), 4096, 0o755, HOUR);
    write(&target.join("debug/deps/libnorth-1.rlib"), 4096, 0o755, HOUR);
    write(&target.join("debug/deps/libnorth-1.rmeta"), 4096, 0o644, HOUR);
    write(&target.join("debug/incremental/north-x/s-1/dep-graph.bin"), 4096, 0o644, HOUR);
    write(&target.join("debug/deps/libfresh.rlib"), 4096, 0o644, Duration::from_secs(30));
    assert_eq!(sweep_now(&target, 12288), 8192);
    assert!(!target.join("debug/deps/north-1").exists());
    assert!(!target.join("debug/incremental/north-x").exists());
    assert!(target.join("debug/deps/libnorth-1.rlib").is_file());
    assert!(target.join("debug/deps/libnorth-1.rmeta").is_file());
}

#[test]
fn an_executable_touched_within_twenty_minutes_stays() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("debug/deps/north-1"), 4096, 0o755, Duration::from_secs(60));
    assert_eq!(sweep_now(&target, 0), 0);
    assert!(target.join("debug/deps/north-1").is_file());
}

#[test]
fn an_incremental_directory_with_one_fresh_file_stays_whole() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("debug/incremental/north-x/old.bin"), 4096, 0o644, HOUR);
    write(&target.join("debug/incremental/north-x/new.bin"), 4096, 0o644, Duration::from_secs(30));
    assert_eq!(sweep_now(&target, 0), 0);
    assert!(target.join("debug/incremental/north-x/old.bin").is_file());
}

#[test]
fn the_sweep_deletes_only_until_the_target_is_back_under_the_cap() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("debug/deps/oldest"), 4096, 0o755, HOUR * 2);
    write(&target.join("debug/deps/newer"), 4096, 0o755, HOUR);
    assert_eq!(sweep_now(&target, 1 << 20), 0);
    assert!(target.join("debug/deps/oldest").is_file());
    assert_eq!(sweep_now(&target, 4096), 4096);
    assert!(!target.join("debug/deps/oldest").exists());
    assert!(target.join("debug/deps/newer").is_file());
}

#[test]
fn only_the_cargo_target_caches_are_swept() {
    let scratch = tempfile::tempdir().unwrap();
    for name in ["cargo-target", "cargo-target-ci", "cargo-targets", "nix"] {
        write(&scratch.path().join(name).join("debug/deps/north-1"), 4096, 0o755, HOUR);
    }
    let names: Vec<_> = shared_targets(scratch.path())
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["cargo-target", "cargo-target-ci"]);
}

#[test]
fn an_executable_hard_linked_elsewhere_in_the_target_stays_and_counts_once() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("debug/deps/north-1"), 4096, 0o755, HOUR);
    fs::hard_link(target.join("debug/deps/north-1"), target.join("debug/north")).unwrap();
    assert_eq!(tree(&target, &mut HashSet::new()).unwrap().0, 4096);
    write(&target.join("debug/fresh.bin"), 1, 0o644, Duration::from_secs(30));
    assert_eq!(sweep_now(&target, 0), 0);
    assert!(target.join("debug/deps/north-1").is_file());
}

#[test]
fn a_named_target_under_the_shared_cache_and_its_release_profile_are_swept() {
    let scratch = tempfile::tempdir().unwrap();
    let nested = scratch.path().join("cargo-target/north");
    write(&nested.join("release/deps/north-1"), 4096, 0o755, HOUR);
    write(&nested.join("debug/incremental/north-x/s-1/dep-graph.bin"), 4096, 0o644, HOUR);
    assert_eq!(shared_targets(scratch.path()), [scratch.path().join("cargo-target"), nested.clone()]);
    assert_eq!(sweep_now(&nested, 0), 8192);
    assert!(!nested.join("release/deps/north-1").exists());
    assert!(!nested.join("debug/incremental/north-x").exists());
}

#[test]
fn an_idle_profile_goes_whole_only_when_nothing_finer_gets_under_the_cap() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("release/deps/libold.rlib"), 4096, 0o644, HOUR * 3);
    write(&target.join("release/build/out.bin"), 4096, 0o644, HOUR * 3);
    write(&target.join("debug/deps/libnew.rlib"), 4096, 0o644, Duration::from_secs(30));
    assert_eq!(sweep_now(&target, 4096), 8192);
    assert!(!target.join("release").exists());
    assert!(target.join("debug/deps/libnew.rlib").is_file());
    assert_eq!(sweep_now(&target, 0), 0);
    assert!(target.join("debug/deps/libnew.rlib").is_file());
}

#[test]
fn an_idle_dependency_artifact_goes_oldest_first_from_a_profile_a_build_just_touched() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("release/deps/libold-1.rlib"), 4096, 0o644, HOUR * 3);
    write(&target.join("release/deps/libold-1.rmeta"), 4096, 0o644, HOUR * 2);
    write(&target.join("release/deps/libold-1.d"), 4096, 0o644, HOUR);
    write(&target.join("release/deps/libfresh-2.rlib"), 4096, 0o644, Duration::from_secs(30));
    assert_eq!(sweep_now(&target, 8192), 8192);
    assert!(!target.join("release/deps/libold-1.rlib").exists());
    assert!(!target.join("release/deps/libold-1.rmeta").exists());
    assert!(target.join("release/deps/libold-1.d").is_file());
    assert!(target.join("release/deps/libfresh-2.rlib").is_file());
    assert_eq!(sweep_now(&target, 0), 4096);
    assert!(!target.join("release/deps/libold-1.d").exists());
    assert!(target.join("release/deps/libfresh-2.rlib").is_file());
}

#[test]
fn a_dependency_artifact_touched_within_twenty_minutes_stays_even_over_the_cap() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("release/deps/libfresh-2.rlib"), 4096, 0o644, Duration::from_secs(60));
    write(&target.join("release/deps/libfresh-2.rmeta"), 4096, 0o644, Duration::from_secs(60));
    assert_eq!(sweep_now(&target, 0), 0);
    assert!(target.join("release/deps/libfresh-2.rlib").is_file());
    assert!(target.join("release/deps/libfresh-2.rmeta").is_file());
}

#[test]
fn a_profile_whose_build_lock_is_held_keeps_its_idle_dependency_artifacts() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("release/deps/libold-1.rlib"), 4096, 0o644, HOUR * 3);
    write(&target.join("release/.cargo-lock"), 0, 0o644, HOUR * 3);
    write(&target.join("debug/deps/libold-2.rlib"), 4096, 0o644, HOUR * 3);
    write(&target.join("debug/fresh.bin"), 1, 0o644, Duration::from_secs(30));
    let building = File::open(target.join("release/.cargo-lock")).unwrap();
    building.lock().unwrap();
    assert_eq!(sweep_now(&target, 0), 4096);
    assert!(target.join("release/deps/libold-1.rlib").is_file());
    assert!(!target.join("debug/deps/libold-2.rlib").exists());
    drop(building);
    assert_eq!(sweep_now(&target, 0), 4096);
    assert!(!target.join("release/deps/libold-1.rlib").exists());
}

#[test]
fn a_small_file_counts_the_disk_blocks_it_occupies_toward_the_cap() {
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("cargo-target");
    write(&target.join("release/deps/libtiny.rlib"), 1, 0o644, HOUR);
    assert_eq!(tree(&target, &mut HashSet::new()).unwrap().0, 1);
    assert_eq!(profile_size(&target).unwrap(), 4096);
    assert_eq!(sweep_now(&target, 0), 4096);
}
