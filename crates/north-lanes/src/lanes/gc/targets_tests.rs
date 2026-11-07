use std::fs;

use super::targets::{BuildTarget, drop_build_target};
use crate::lanes::test_git::{git_in, init_repo};

#[test]
fn a_kept_lane_loses_its_ignored_build_target_unless_a_process_holds_it_open() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let main = root.join("main");
    init_repo(&main, None);
    let lane = root.join("lane");
    git_in(&main, &["worktree", "add", "-q", "--detach", lane.to_str().unwrap()]);
    let no_processes = root.join("proc");
    fs::create_dir_all(&no_processes).unwrap();
    fs::create_dir_all(lane.join("target/debug")).unwrap();
    fs::write(lane.join("target/debug/out"), vec![7u8; 64 * 1024]).unwrap();

    assert_eq!(drop_build_target(&lane, &[], &no_processes, false), BuildTarget::Absent, "a target the repository does not ignore is work, not build output");

    fs::write(main.join(".git/info/exclude"), "target/\n").unwrap();
    let open = vec![lane.join("target/debug/.cargo-lock")];
    assert_eq!(drop_build_target(&lane, &open, &no_processes, false), BuildTarget::InUse);
    assert!(lane.join("target/debug/out").exists());

    assert!(matches!(drop_build_target(&lane, &[], &no_processes, true), BuildTarget::Dropped(bytes) if bytes >= 64 * 1024));
    assert!(lane.join("target").exists(), "a dry run removes nothing");

    assert!(matches!(drop_build_target(&lane, &[], &no_processes, false), BuildTarget::Dropped(bytes) if bytes >= 64 * 1024));
    assert!(!lane.join("target").exists());
    assert!(lane.join(".git").exists(), "the worktree itself stays");
}
