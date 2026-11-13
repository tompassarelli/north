use std::fs;
use std::os::unix::fs::PermissionsExt;
use super::archive::{CLOSED_ISSUE_HOURS, Lane, archive, archive_unregistered, broken, note_broken, retire_broken, window_hours};
use super::evidence::IssueState;
use crate::lanes::test_git::{git_in, init_repo};

#[test]
fn a_closed_issue_lane_is_bundled_then_reaped_after_its_window() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let main = root.join("proj/main");
    init_repo(&main, None);
    let path = root.join("proj/worktrees/old");
    git_in(&main, &["worktree", "add", "-q", "-b", "old", path.to_str().unwrap(), "main"]);
    fs::write(path.join("work.txt"), "committed work\n").unwrap();
    git_in(&path, &["add", "work.txt"]);
    git_in(&path, &["commit", "-qm", "work (proj#7)"]);
    fs::write(path.join("notes.txt"), "uncommitted notes\n").unwrap();
    let tip = git_in(&path, &["rev-parse", "HEAD"]);
    let record = main.join(".git/north/lane-issues/old");
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    fs::write(&record, "proj#7\n").unwrap();

    assert_eq!(window_hours(&[IssueState::Open]), None, "an open issue keeps its lane");
    assert_eq!(window_hours(&[IssueState::Closed, IssueState::Unknown]), None);
    let window = window_hours(&[IssueState::Closed]).unwrap();
    assert_eq!(window, CLOSED_ISSUE_HOURS);
    assert!(window_hours(&[]).unwrap() > window, "a lane naming no issue waits longer");

    let archive_root = root.join("archive/lanes");
    let issues = vec!["proj#7".to_string()];
    let lane = Lane { main: &main, base: "main", project: "proj", worktree: Some(&path), branch: "old", tip: &tip, issues: &issues, why: "issue closed: proj#7" };
    let bundle = archive(&lane, &archive_root, 0).unwrap();

    assert!(bundle.starts_with(archive_root.join("proj")));
    git_in(&main, &["bundle", "verify", "-q", bundle.to_str().unwrap()]);
    assert!(!path.exists(), "the lane is reaped once its bundle verifies");
    assert!(git_in(&main, &["branch", "--list", "old"]).is_empty());
    assert!(!record.exists(), "the issue record goes with its branch");
    assert!(fs::read_to_string(archive_root.join("proj/index.tsv")).unwrap().contains("proj#7"));

    let restored = root.join("restored");
    git_in(&root, &["clone", "-q", main.to_str().unwrap(), restored.to_str().unwrap()]);
    git_in(&restored, &["fetch", "-q", bundle.to_str().unwrap(), "refs/north-archive/old:refs/heads/old"]);
    git_in(&restored, &["checkout", "-q", "old"]);
    assert_eq!(fs::read_to_string(restored.join("work.txt")).unwrap(), "committed work\n");
    assert_eq!(fs::read_to_string(restored.join("notes.txt")).unwrap(), "uncommitted notes\n", "uncommitted changes ride in the bundle");
    assert_eq!(git_in(&restored, &["rev-parse", "HEAD^"]), tip);
}

#[test]
fn a_half_removed_lane_is_archived_once_and_a_branch_missing_objects_is_recorded_broken_once() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let main = root.join("proj/main");
    init_repo(&main, None);
    let common = main.join(".git");
    let directory = root.join("proj/worktrees/half");
    let archive_root = root.join("archive/lanes");
    let lane = Lane { main: &main, base: "main", project: "proj", worktree: Some(&directory), branch: "", tip: "", issues: &[], why: "git no longer registers this lane directory" };
    let leave = || {
        let locked = directory.join("tools/modules");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("tsc.js"), "left behind\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(directory.join("tools"), fs::Permissions::from_mode(0o555)).unwrap();
    };
    let bundles = || fs::read_dir(archive_root.join("proj")).unwrap().flatten().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "bundle")).count();

    leave();
    let first = archive_unregistered(&lane, &directory, &common, &archive_root, 0).unwrap();
    assert!(!directory.exists(), "a directory of read-only folders is still removed once bundled");

    leave();
    let second = archive_unregistered(&lane, &directory, &common, &archive_root, 3_600_000).unwrap();
    assert_eq!(first, second);
    assert_eq!(bundles(), 1, "the same files are bundled once");
    assert_eq!(fs::read_to_string(archive_root.join("proj/index.tsv")).unwrap().lines().count(), 1, "and indexed once");

    git_in(&main, &["checkout", "-q", "-b", "lost"]);
    fs::write(main.join("lost.txt"), "work whose blob is gone\n").unwrap();
    git_in(&main, &["add", "lost.txt"]);
    git_in(&main, &["commit", "-qm", "lost work"]);
    let blob = git_in(&main, &["rev-parse", "HEAD:lost.txt"]);
    let tip = git_in(&main, &["rev-parse", "HEAD"]);
    git_in(&main, &["checkout", "-q", "main"]);
    fs::remove_file(main.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
    let index = || fs::read_to_string(archive_root.join("proj/index.tsv")).unwrap();
    let stuck = Lane { main: &main, base: "main", project: "proj", worktree: None, branch: "lost", tip: &tip, issues: &[], why: "no issue, no owner" };
    let found = broken(&stuck, &archive_root).unwrap();
    assert!(!found.recorded, "{}", found.detail);
    assert!(archive(&stuck, &archive_root, 0).unwrap_err().starts_with("broken ref"), "no bundle is attempted");
    note_broken(&stuck, &archive_root, &found, 0).unwrap();
    assert!(broken(&stuck, &archive_root).unwrap().recorded, "the next run skips it");
    assert_eq!(bundles(), 1);
    assert_eq!(index().lines().count(), 2);

    let kept = Lane { why: "rejected: kept in /archive/lost.tar", ..stuck };
    retire_broken(&kept, &archive_root, &found, 0).unwrap();
    assert!(git_in(&main, &["branch", "--list", "lost"]).is_empty());
    assert!(index().lines().last().unwrap().contains("rejected: kept in /archive/lost.tar; broken ref"));
}
