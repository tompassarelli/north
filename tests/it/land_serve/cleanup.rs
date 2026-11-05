use super::*;

fn exists(fixture: &Fixture, lane: &Path) -> (bool, bool) {
    let branch = lane.file_name().unwrap().to_str().unwrap();
    let held = Command::new("git").arg("-C").arg(&fixture.repo).args(["rev-parse", "-q", "--verify", &format!("refs/heads/lane-{branch}")]).output().unwrap().status.success();
    (lane.exists(), held)
}

fn remote_has(fixture: &Fixture, branch: &str) -> bool {
    Command::new("git").arg("-C").arg(&fixture.origin).args(["rev-parse", "-q", "--verify", &format!("refs/heads/{branch}")]).output().unwrap().status.success()
}

fn landed_lane(fixture: &mut Fixture) -> PathBuf {
    fixture.lane("a", "a.txt", "a")
}

#[test]
fn clean() {
    let mut fixture = Fixture::new();
    fs::write(fixture.repo.join(".git/info/exclude"), "target/\n").unwrap();
    let lane = landed_lane(&mut fixture);
    fs::create_dir_all(lane.join("target/debug")).unwrap();
    fs::write(lane.join("target/debug/bulk.rlib"), vec![0u8; 1 << 20]).unwrap();
    fixture.enqueue(&lane, None);
    assert!(fixture.serve().status.success());
    assert_eq!(exists(&fixture, &lane), (false, false));
    assert_eq!(git(&fixture.origin, &["show", "main:a.txt"]), "a");
    let worktrees = git(&fixture.repo, &["worktree", "list", "--porcelain"]);
    assert!(!worktrees.contains("worktrees/a\n"), "{worktrees}");
}

#[test]
fn pid_namespace() {
    let mut fixture = Fixture::new();
    let lane = landed_lane(&mut fixture);
    fixture.enqueue(&lane, None);
    let mut occupant = Command::new("bwrap")
        .args(["--unshare-user", "--unshare-pid", "--die-with-parent", "--dev-bind", "/", "/", "--tmpfs", "/mnt", "--bind"])
        .arg(&lane)
        .args(["/mnt/lane", "--chdir", "/mnt/lane", "--", "sleep", "120"])
        .current_dir("/")
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(occupant.try_wait().unwrap().is_none(), "the sandboxed occupant exited before the lander ran");
    assert!(fixture.serve().status.success());
    let _ = occupant.kill();
    let _ = occupant.wait();
    assert_eq!(exists(&fixture, &lane), (true, true), "a lane with a process in it under another PID namespace was removed");
}

#[test]
fn keep() {
    let mut fixture = Fixture::new();
    let lane = landed_lane(&mut fixture);
    let path = fixture.enqueue(&lane, None);
    let mut ticket = queue::read(&path).unwrap();
    ticket.keep_lane = true;
    queue::write(&path, &ticket).unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(exists(&fixture, &lane), (true, true), "--keep-lane removed the lane");
}

#[test]
fn dirty() {
    let mut fixture = Fixture::new();
    let lane = landed_lane(&mut fixture);
    fixture.enqueue(&lane, None);
    fs::write(lane.join("notes.txt"), "unsaved\n").unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(exists(&fixture, &lane), (true, true));
    assert!(lane.join("notes.txt").is_file());
}

#[test]
fn remote_merged() {
    let mut fixture = Fixture::new();
    let lane = landed_lane(&mut fixture);
    git(&lane, &["push", "-q", "origin", "lane-a"]);
    fixture.enqueue(&lane, None);
    assert!(fixture.serve().status.success());
    assert!(!remote_has(&fixture, "lane-a"), "the merged remote lane branch was not deleted");
    assert_eq!(exists(&fixture, &lane), (false, false));
}

#[test]
fn remote_unmerged() {
    let mut fixture = Fixture::new();
    let lane = landed_lane(&mut fixture);
    fixture.commit(&lane, "extra.txt", "only on the remote branch", "extra");
    git(&lane, &["push", "-q", "origin", "lane-a"]);
    git(&lane, &["reset", "-q", "--hard", "HEAD~1"]);
    fixture.enqueue(&lane, None);
    assert!(fixture.serve().status.success());
    assert!(remote_has(&fixture, "lane-a"), "a remote lane branch with unlanded work was deleted");
}
