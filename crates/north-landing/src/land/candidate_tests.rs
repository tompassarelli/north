use super::*;

#[test]
fn ensure_writes_no_merge_driver_into_the_shared_config_and_every_call_carries_the_drivers() {
    merge_set::run_drivers_with(Path::new("false"));
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    git(&repo, &["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "base"]).unwrap();
    let worktree = dir.path().join("lander");
    ensure(&repo, &worktree).unwrap();
    let shared = Command::new("git").arg("-C").arg(&worktree).args(["config", "--local", "--get-regexp", "^merge\\."]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&shared.stdout), "");
    assert_eq!(git(&worktree, &["config", "merge.set.driver"]).unwrap(), "'false' merge-driver set %O %A %B %P");
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
        let ticket = Ticket { root: repo.clone(), branch: note.to_owned(), head, ..Default::default() };
        tickets.push((dir.path().join(format!("{note}.json")), ticket));
    }
    let worktree = dir.path().join("lander");
    ensure(&repo, &worktree).unwrap();
    let built = build(&worktree, BRANCH, &base, &tickets, 2, &dir.path().join("logs")).unwrap();
    assert_eq!(built.included(), 2);
    assert_eq!(git(&worktree, &["show", "HEAD:index.txt"]).unwrap(), "a\nb\nc");
    assert_eq!(git(&worktree, &["show", "HEAD~1:index.txt"]).unwrap(), "a\nb");
}

fn lane(repo: &Path, base: &str, name: &str, files: &[(&str, &str)]) -> Ticket {
    git(repo, &["checkout", "-q", "-b", name, base]).unwrap();
    for (path, text) in files {
        fs::write(repo.join(path), text).unwrap();
    }
    commit_all(repo, name);
    let head = git(repo, &["rev-parse", "HEAD"]).unwrap();
    Ticket { origin: String::new(), root: repo.to_owned(), host: String::new(), branch: name.to_owned(), head, session: None, queued_at: String::new(), first_commit_secs: None, several_commits: None, red_fix: None, verdict: None, keep_lane: false }
}

#[test]
fn a_merge_set_generator_change_defers_a_later_lane_on_its_sources_and_not_a_lane_beside_them() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("notes")).unwrap();
    fs::create_dir_all(repo.join("harness")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    fs::write(repo.join("harness/merge-sets.toml"), "[[set]]\npath = \"index.txt\"\nkind = \"regenerate\"\ncommand = \"sh gen.sh\"\nsources = [\"notes/\"]\ngenerator = [\"gen.sh\"]\n").unwrap();
    fs::write(repo.join("gen.sh"), "ls notes > index.txt\n").unwrap();
    fs::write(repo.join("notes/a"), "a\n").unwrap();
    fs::write(repo.join("index.txt"), "a\n").unwrap();
    fs::write(repo.join("other.txt"), "o\n").unwrap();
    commit_all(&repo, "base");
    let base = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let tickets = [
        lane(&repo, &base, "generator", &[("gen.sh", "ls -r notes > index.txt\n")]),
        lane(&repo, &base, "source", &[("notes/b", "b\n")]),
        lane(&repo, &base, "beside", &[("other.txt", "p\n")]),
    ]
    .into_iter()
    .map(|ticket| (dir.path().join(format!("{}.json", ticket.branch)), ticket))
    .collect::<Vec<_>>();
    let worktree = dir.path().join("lander");
    ensure(&repo, &worktree).unwrap();
    let built = build(&worktree, BRANCH, &base, &tickets, 3, &dir.path().join("logs")).unwrap();
    assert!(matches!(&built.picks[..], [Pick::Included, Pick::Deferred(reason), Pick::Included] if reason.starts_with("source: ") && reason.contains("index.txt")));
    assert_eq!(git(&worktree, &["show", "HEAD:index.txt"]).unwrap(), "a");
    assert_eq!(git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]).unwrap(), "2");
}
