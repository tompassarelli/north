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
fn emptied_pick_a_lane_whose_change_is_already_carried_is_named_by_its_carrier_or_deferred_or_refused() {
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
    emptied_pick_scenario(&dir.path().join("emptied"));
    rebase_compile_scenario(&dir.path().join("rebase"));
}

fn lanes_repo(dir: &Path) -> (PathBuf, String) {
    let repo = dir.join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    fs::write(repo.join("f"), "A0\nC\nB0\n").unwrap();
    commit_all(&repo, "base");
    let base = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    (repo, base)
}

fn lane_editing(repo: &Path, base: &str, branch: &str, text: &str) -> String {
    git(repo, &["checkout", "-q", "-B", branch, base]).unwrap();
    fs::write(repo.join("f"), text).unwrap();
    commit_all(repo, "Edit the lines of the lane");
    let head = git(repo, &["rev-parse", "HEAD"]).unwrap();
    git(repo, &["checkout", "-q", "main"]).unwrap();
    head
}

fn lane_ticket(repo: &Path, dir: &Path, branch: &str, head: &str) -> (PathBuf, Ticket) {
    (dir.join(format!("{branch}.json")), Ticket { branch: branch.to_owned(), head: head.to_owned(), root: repo.to_path_buf(), origin: repo.display().to_string(), ..Default::default() })
}

fn emptied_pick_scenario(dir: &Path) {
    let (repo, base) = lanes_repo(dir);
    let first = lane_editing(&repo, &base, "lane-one", "A1\nC\nB1\n");
    let second = lane_editing(&repo, &base, "lane-two", "A1\nC\nB0\n");
    let worktree = dir.join("worktrees/lander");
    ensure(&repo, &worktree).unwrap();
    let logs = dir.join("logs");
    let both = [lane_ticket(&repo, dir, "lane-one", &first), lane_ticket(&repo, dir, "lane-two", &second)];
    let built = build(&worktree, &base, &both, &[], usize::MAX, &logs).unwrap();
    assert!(matches!(built.picks[0], Pick::Included));
    assert!(matches!(built.picks[1], Pick::Deferred(_)), "lane-two's edit to A is already in the candidate through lane-one, so it waits for the next candidate");
    fs::write(repo.join("f"), "A1\nC\nB1\n").unwrap();
    commit_all(&repo, "Land lane-one");
    let landed = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let alone = [lane_ticket(&repo, dir, "lane-two", &second)];
    let built = build(&worktree, &landed, &alone, &[], usize::MAX, &logs).unwrap();
    assert!(matches!(built.picks[0], Pick::Decided(_)));
    assert!(logs.join("lane-two.dropped.log").exists(), "a lane whose change the base already holds is refused as dropped, not queued again");
}

#[test]
fn a_candidate_of_ten_lanes_runs_its_pick_time_gates_outside_the_serving_loop() {
    let jobs: Vec<GateJob> = (0..10).map(|at| GateJob { name: format!("lane-{at}"), gating: Gating { base: String::new(), checked_on: None, picked_onto: String::new(), head: String::new() } }).collect();
    let together = std::sync::Barrier::new(5);
    let judged = gate_lanes_with(&jobs, 5, |job| {
        together.wait();
        if job.name == "lane-3" { Err("lane-3".to_owned()) } else { Ok(None) }
    });
    assert_eq!(judged.len(), 10);
    assert_eq!(judged[3].as_ref().err().map(String::as_str), Some("lane-3"), "each verdict stays with its lane");
}

fn rebase_compile_scenario(dir: &Path) {
    let repo = dir.join("main");
    for (name, manifest, source) in [("a", "", "pub struct Context { pub user: u32 }\n"), ("b", "a = { path = \"../a\" }\n", "pub fn context() -> a::Context { a::Context { user: 1 } }\n")] {
        fs::create_dir_all(repo.join(format!("crates/{name}/src"))).unwrap();
        fs::write(repo.join(format!("crates/{name}/Cargo.toml")), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{manifest}")).unwrap();
        fs::write(repo.join(format!("crates/{name}/src/lib.rs")), source).unwrap();
    }
    fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\nresolver = \"3\"\n").unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    commit_all(&repo, "base");
    let checked_on = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    fs::write(repo.join("crates/a/src/lib.rs"), "pub struct Context { pub user: u32, pub user_runtime: u32 }\n").unwrap();
    commit_all(&repo, "Add a field to the context");
    let moved = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    fs::write(repo.join("crates/b/src/lib.rs"), "pub fn context() -> a::Context { a::Context { user: 2 } }\n").unwrap();
    commit_all(&repo, "Build the context in the lane");
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let tree = dir.join("tree");
    git(&repo, &["checkout", "-q", "--detach", &moved]).unwrap();
    git(&repo, &["worktree", "add", "-q", "--detach", tree.to_str().unwrap(), &head]).unwrap();
    fs::create_dir_all(tree.join(".cargo")).unwrap();
    fs::write(tree.join(".cargo/config.toml"), "[build\nrustc = \"the lane's own program\"\n").unwrap();
    let north = dir.join("north");
    fs::write(&north, format!("#!/bin/sh\necho \"$NORTH_OFFLOAD_LANDER_SLOT $*\" >> {}\ncase \"$3\" in\n  choose) echo build-host-one ;;\n  run) printf 'error[E0063]: missing field `user_runtime` in initializer of `a::Context`\\n --> crates/b/src/lib.rs:1:35\\nerror: could not compile `b`\\n' >&2; exit 101 ;;\nesac\n", dir.join("asked").display())).unwrap();
    fs::set_permissions(&north, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let logs = dir.join("logs");
    let unmoved = Rebased { tree: &repo, checked_on: Some(&moved), picked_onto: &moved, head: &head, shipped_from: &moved };
    assert!(rebase_compile::judge(&unmoved, &north, &logs, "lane").unwrap().is_none() && !dir.join("asked").exists(), "a lane checked on the main it is picked onto compiles nothing");
    let rebased = Rebased { tree: &tree, checked_on: Some(&checked_on), picked_onto: &moved, head: &head, shipped_from: &moved };
    let refused = rebase_compile::judge(&rebased, &north, &logs, "lane").unwrap().expect("the lane rebased onto the moved main does not compile, its manifest read from the repository's main checkout, where neither the lane's cargo config, which cargo cannot parse, nor its flake's dev shell applies");
    assert_eq!(refused.gates, crate::landing_refusals::REBASE_COMPILE);
    assert!(refused.verdict.contains("error[E0063]: missing field `user_runtime`") && refused.verdict.contains("--> crates/b/src/lib.rs:1:35"), "on 2026-11-13 80b143969c, checked on an older main, was pushed onto 68f3121884 without a compile and main stayed red with E0063: {}", refused.verdict);
    assert!(fs::read_to_string(dir.join("asked")).unwrap().contains("cargo check --all-targets --locked --offline -p a -p b"), "the base changed crate a, which the lane's crate b depends on, so both are checked");
    assert!(fs::read_to_string(dir.join("asked")).unwrap().contains("1 capacity offload run --host build-host-one"), "on 2026-11-13 four of nine compiles after a rebase waited in the build host's line behind worker builds until the 110 s limit and landed with no verdict; the lander's compile takes its first-in-line slot");
    let offline = dir.join("offline");
    fs::write(&offline, "#!/bin/sh\ncase \"$3\" in\n  choose) echo build-host-one ;;\n  run) echo 'error: no matching package named `a` found' >&2; exit 101 ;;\nesac\n").unwrap();
    fs::set_permissions(&offline, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(rebase_compile::judge(&rebased, &offline, &logs, "lane").unwrap().is_none(), "cargo exits 101 for a resolver failure too, which says nothing about the lane");
    fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nthiserror = \"2\"\nthiserror = \"2\"\n").unwrap();
    commit_all(&repo, "Add thiserror to the workspace a second time");
    let unreadable = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let plain = dir.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let no_cargo = Rebased { tree: &plain, checked_on: Some(&checked_on), picked_onto: &moved, head: &head, shipped_from: &moved };
    assert!(rebase_compile::judge(&no_cargo, &offline, &logs, "lane").unwrap().is_none(), "a repository with no Cargo.toml, such as nixos-config, has no manifest for cargo to read");
    let broken = Rebased { tree: &repo, checked_on: Some(&checked_on), picked_onto: &moved, head: &unreadable, shipped_from: &moved };
    let refused = rebase_compile::judge(&broken, &offline, &logs, "lane").unwrap().expect("a rebased tree whose manifest cargo cannot read is refused");
    assert!(refused.verdict.contains("duplicate key") && refused.verdict.contains("--> Cargo.toml:7:1"), "on 2026-11-13 e62678a196 left main's Cargo.toml with a second thiserror key; the compile after the rebase could not read which crates it reached and landed it with no verdict: {}", refused.verdict);
    let reverted = Rebased { checked_on: None, ..broken };
    assert!(rebase_compile::judge(&reverted, &offline, &logs, "sheriff-revert").unwrap().is_some_and(|refused| refused.verdict.contains("duplicate key")), "on 2026-11-13 the sheriff's queued revert 7ba951faca, a ticket checked on no base, dropped the workspace's thiserror entry while north-graph still inherited it, and landed past the manifest read");
}
