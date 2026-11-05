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

