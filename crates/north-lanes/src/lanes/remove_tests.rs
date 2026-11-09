use std::path::{Path, PathBuf};

use super::remove::remove;
use super::test_git::{git_in, init_repo};

fn nested_lane(home: &Path) -> (PathBuf, PathBuf) {
    let main = home.join("code/proj/main");
    init_repo(&main, None);
    let lane = main.join("proj/worktrees/config-drift-993");
    git_in(&main, &["worktree", "add", "-q", "--detach", lane.to_str().unwrap(), "HEAD"]);
    (main, lane)
}

#[test]
fn an_empty_shell_nested_inside_the_main_checkout_is_removed_north_1006() {
    let home = tempfile::tempdir().unwrap();
    let (main, lane) = nested_lane(home.path());
    std::fs::remove_file(lane.join("f.txt")).unwrap();
    git_in(&main, &["worktree", "lock", lane.to_str().unwrap()]);

    let queues = home.path().join("queues");
    assert!(remove(&lane, &queues).is_ok());
    assert!(!lane.exists() && !main.join("proj").exists());
    assert!(!git_in(&main, &["worktree", "list"]).contains("config-drift-993"));
}

#[test]
fn a_nested_lane_that_holds_files_is_kept_north_1006() {
    let home = tempfile::tempdir().unwrap();
    let (_, lane) = nested_lane(home.path());

    let kept = remove(&lane, &home.path().join("queues")).unwrap_err();
    assert!(kept.contains("worktree move"), "{kept}");
    assert!(lane.join("f.txt").exists());
}
