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

#[test]
fn a_lane_whose_change_main_already_carries_is_named_by_the_main_commit_carrying_it_not_by_the_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    fs::write(repo.join("a"), "a\n").unwrap();
    commit_all(&repo, "base");
    git(&repo, &["checkout", "-q", "-b", "model-catalog-argon"]).unwrap();
    fs::write(repo.join("argon"), "argon\n").unwrap();
    commit_all(&repo, "Add the Argon efforts");
    let duplicate = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    git(&repo, &["checkout", "-q", "main"]).unwrap();
    fs::write(repo.join("argon"), "argon\n").unwrap();
    commit_all(&repo, "Land the Argon efforts from another lane");
    let carrier = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    fs::write(repo.join("clock"), "clock\n").unwrap();
    commit_all(&repo, "Name the wall clock");
    let testclock = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let ticket = |branch: &str, head: &str| (dir.path().join(format!("{branch}.json")), Ticket { branch: branch.to_owned(), head: head.to_owned(), root: repo.clone(), origin: repo.display().to_string(), ..Default::default() });
    let worktree = dir.path().join("worktrees/lander");
    ensure(&repo, &worktree).unwrap();
    let built = build(&worktree, &testclock, &[ticket("model-catalog-argon", &duplicate)], &[], usize::MAX, &dir.path().join("logs")).unwrap();
    let decided = match &built.picks[0] { Pick::Decided(text) => text.clone(), _ => String::new() };
    assert_eq!(decided, format!("landed {carrier}"), "on 2026-11-08 model-catalog-argon, whose change da2941ea10 had already landed, was told it landed as bde3db9c68, the base it was checked against");
}

#[test]
fn a_candidate_of_ten_lanes_runs_its_pick_time_gates_outside_the_serving_loop() {
    let jobs: Vec<GateJob> = (0..10).map(|at| GateJob { name: format!("lane-{at}"), gating: Gating { picked_onto: String::new(), head: String::new() } }).collect();
    let together = std::sync::Barrier::new(5);
    let judged = gate_lanes_with(&jobs, 5, |job| {
        together.wait();
        if job.name == "lane-3" { Err("lane-3".to_owned()) } else { Ok(None) }
    });
    assert_eq!(judged.len(), 10);
    assert_eq!(judged[3].as_ref().err().map(String::as_str), Some("lane-3"), "each verdict stays with its lane");
}
