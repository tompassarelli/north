use super::*;

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]).unwrap();
    git(root, &["commit", "-q", "-m", message]).unwrap();
}

#[test]
fn a_moved_lander_worktree_builds_while_its_old_worktree_still_holds_the_candidate_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    fs::write(repo.join("a"), "a\n").unwrap();
    commit_all(&repo, "base");
    let base = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let old = dir.path().join("worktrees/worktrees/lander");
    ensure(&repo, &old).unwrap();
    build(&old, &base, &[], &[], 1, &dir.path().join("logs")).unwrap();
    git(&old, &["checkout", "-q", "-B", "land-candidate", &base]).unwrap();
    let moved = dir.path().join("worktrees/lander");
    ensure(&repo, &moved).unwrap();
    let built = build(&moved, &base, &[], &[], 1, &dir.path().join("logs")).unwrap();
    assert_eq!(built.included(), 0);
    assert_eq!(git(&moved, &["rev-parse", "HEAD"]).unwrap(), base);
}
