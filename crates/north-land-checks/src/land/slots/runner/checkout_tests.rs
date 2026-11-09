use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::BaseTree;

fn git_text(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(arguments).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn lane() -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lane");
    fs::create_dir_all(&root).unwrap();
    git_text(&root, &["init", "-q", "--initial-branch=main"]);
    git_text(&root, &["config", "user.email", "land@north"]);
    git_text(&root, &["config", "user.name", "land"]);
    fs::write(root.join("lane.txt"), "base\n").unwrap();
    git_text(&root, &["add", "lane.txt"]);
    git_text(&root, &["commit", "-q", "-m", "base"]);
    let base = git_text(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("lane.txt"), "head\n").unwrap();
    git_text(&root, &["commit", "-q", "-am", "head"]);
    (dir, root, base)
}

fn registered_worktrees(root: &Path) -> usize {
    git_text(root, &["worktree", "list", "--porcelain"]).lines().filter(|line| line.starts_with("worktree ")).count()
}

#[test]
fn a_base_rerun_takes_a_worktree_and_a_target_of_its_own_while_the_lane_keeps_its_head() {
    let (dir, root, base) = lane();
    let head = git_text(&root, &["rev-parse", "HEAD"]);
    let tree = BaseTree::detached_in(&root, &base, dir.path()).unwrap();
    let beside = tree.path().to_path_buf();
    assert_ne!(beside, root);
    assert!(!beside.starts_with(&root), "the rerun shares the lane's working tree: {}", beside.display());
    assert_ne!(beside.join("target"), root.join("target"), "the rerun shares the lane's target directory");
    assert_eq!(fs::read_to_string(beside.join("lane.txt")).unwrap(), "base\n");
    assert_eq!(fs::read_to_string(root.join("lane.txt")).unwrap(), "head\n");
    assert_eq!(git_text(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(registered_worktrees(&root), 2);
    drop(tree);
    assert!(!beside.exists(), "the rerun leaves its worktree behind at {}", beside.display());
    assert_eq!(registered_worktrees(&root), 1);
    assert_eq!(fs::read_to_string(root.join("lane.txt")).unwrap(), "head\n");
}
