use super::*;

#[test]
fn ensure_names_the_merge_driver_in_the_lander_worktree_config() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    git(&repo, &["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "base"]).unwrap();
    let worktree = dir.path().join("lander");
    ensure(&repo, &worktree).unwrap();
    assert_eq!(git(&worktree, &["config", "merge.set.driver"]).unwrap(), merge_set::DRIVER_COMMAND);
    assert_eq!(git(&worktree, &["config", "merge.set.name"]).unwrap(), merge_set::DRIVER_NAME);
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]).unwrap();
    git(root, &["commit", "-q", "-m", message]).unwrap();
}

#[test]
fn two_lanes_that_each_add_a_note_without_its_index_build_a_candidate_with_a_fresh_index() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("notes")).unwrap();
    fs::create_dir_all(repo.join("harness")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    fs::write(repo.join("harness/merge-sets.toml"), "[[set]]\npath = \"index.txt\"\nkind = \"regenerate\"\ncommand = \"sh gen.sh\"\nsources = [\"notes/\"]\n").unwrap();
    fs::write(repo.join("gen.sh"), "ls notes > index.txt\n").unwrap();
    fs::write(repo.join("notes/a"), "a\n").unwrap();
    fs::write(repo.join("index.txt"), "a\n").unwrap();
    commit_all(&repo, "base");
    let base = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let mut tickets = Vec::new();
    for note in ["b", "c"] {
        git(&repo, &["checkout", "-q", "-b", note, &base]).unwrap();
        fs::write(repo.join("notes").join(note), "x\n").unwrap();
        commit_all(&repo, note);
        let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
        let ticket = Ticket { origin: String::new(), root: repo.clone(), host: String::new(), branch: note.to_owned(), head, session: None, queued_at: String::new(), first_commit_secs: None, several_commits: None, red_fix: None, verdict: None, keep_lane: false };
        tickets.push((dir.path().join(format!("{note}.json")), ticket));
    }
    let worktree = dir.path().join("lander");
    ensure(&repo, &worktree).unwrap();
    let built = build(&worktree, BRANCH, &base, &tickets, 2, &dir.path().join("logs")).unwrap();
    assert_eq!(built.included(), 2);
    assert_eq!(git(&worktree, &["show", "HEAD:index.txt"]).unwrap(), "a\nb\nc");
    assert_eq!(git(&worktree, &["show", "HEAD~1:index.txt"]).unwrap(), "a\nb");
}
