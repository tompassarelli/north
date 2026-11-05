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
}

