use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};

use super::{device, holds};

/// On 2026-11-12 a records-mirror copy held off by a lock a unit took outlived its reason by two hours and stranded six landing verdicts, and no command named it.
#[test]
fn a_records_mirror_lock_held_by_hand_is_listed_red_with_its_holder() {
    let (state, proc_root) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let lock = state.path().join("records-mirror.lock");
    fs::write(&lock, "").unwrap();
    let metadata = fs::metadata(&lock).unwrap();
    let (major, minor) = device(metadata.dev());
    fs::write(proc_root.path().join("locks"), format!("1: FLOCK  ADVISORY  WRITE 4242 {major:02x}:{minor:02x}:{} 0 EOF\n", metadata.ino())).unwrap();
    fs::write(proc_root.path().join("stat"), "cpu 0\nbtime 1762900000\n").unwrap();
    let holder = proc_root.path().join("4242");
    fs::create_dir(&holder).unwrap();
    symlink("/run/current-system/sw/bin/flock", holder.join("exe")).unwrap();
    fs::write(holder.join("cmdline"), "flock\0records-mirror.lock\0sleep\0infinity\0").unwrap();
    fs::write(holder.join("cgroup"), "0::/user.slice/user-1000.slice/user@1000.service/app.slice/records-mirror-hold.service\n").unwrap();
    fs::write(holder.join("stat"), format!("4242 (flock) S {}\n", ["0"; 18].join(" ") + " 100")).unwrap();

    let listed = holds(state.path(), proc_root.path(), |unit| (unit == "records-mirror-hold.service").then(|| "hold the records mirror for the disk".into()));
    let [hold] = listed.as_slice() else { panic!("{listed:?}") };
    assert_eq!((hold.kind.as_str(), hold.set_by.as_str(), hold.since), ("lock records-mirror.lock held by hand", "records-mirror-hold.service", Some(1_762_900_001)));
    let judged = hold.clone().judged(1_762_900_001 + 600, &|_| false);
    assert_eq!(judged.red.as_deref(), Some("it names nothing that lifts it"), "{judged:?}");
}
