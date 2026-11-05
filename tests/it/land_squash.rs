use north_landing::land::enqueue::{Environment, Queued, Shape, enqueue};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().canonicalize().unwrap();
        git(&path, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&path, &["init", "-q", "-b", "main", "repo"]);
        let repo = path.join("repo");
        git(&repo, &["config", "user.name", "Squash scenario"]);
        git(&repo, &["config", "user.email", "squash@example.invalid"]);
        git(&repo, &["remote", "add", "origin", path.join("origin.git").to_str().unwrap()]);
        write(&repo.join("content.txt"), "initial\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        Self { root, repo }
    }
}

fn lane_commit(fixture: &Fixture, subject: &str, trailers: &[&str]) {
    let path = fixture.repo.join("content.txt");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(subject);
    text.push('\n');
    write(&path, &text);
    let mut arguments = vec!["commit", "-qam", subject];
    for trailer in trailers {
        arguments.extend(["--trailer", trailer]);
    }
    git(&fixture.repo, &arguments);
}

fn queue_lane(fixture: &Fixture, message: Option<&str>) -> Queued {
    git(&fixture.repo, &["config", "landing.lander-host", "laptop"]);
    let state = fixture.root.path().join("state");
    let south = fixture.root.path().join("no-south");
    let environment = Environment { state_home: &state, host: "laptop", session: None, south: &south, denylist: None };
    let queued = enqueue(&fixture.repo, Shape::OneCommit { message }, false, &environment).unwrap();
    git(&fixture.repo, &["config", "--unset", "landing.lander-host"]);
    queued
}

fn message_of(fixture: &Fixture, rev: &str) -> String {
    git(&fixture.repo, &["show", "-s", "--format=%B", rev])
}

fn trailers_of(fixture: &Fixture, rev: &str) -> String {
    git(&fixture.repo, &["show", "-s", "--format=%(trailers:only,unfold)", rev]).lines().filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

#[test]
fn squash_one() {
    let fixture = Fixture::new();
    let base = git(&fixture.repo, &["rev-parse", "origin/main"]);
    lane_commit(&fixture, "outgoing", &["Refs: #7", "Lane-note: first"]);
    lane_commit(&fixture, "add the second lane line", &["Refs: #7", "Lane-note: second"]);
    lane_commit(&fixture, "add the third lane line", &[]);
    let tree = git(&fixture.repo, &["rev-parse", "HEAD^{tree}"]);
    let queued = queue_lane(&fixture, None);
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{base}..HEAD")]), "1", "a three-commit lane did not fold to one commit");
    assert_eq!(queued.ticket.head, git(&fixture.repo, &["rev-parse", "HEAD"]));
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD^"]), base);
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD^{tree}"]), tree, "the folded commit lost the lane tree");
    let message = message_of(&fixture, "HEAD");
    assert_eq!(message.lines().next(), Some("outgoing"), "{message}");
    assert!(message.lines().any(|line| line == "- add the second lane line"), "{message}");
    assert_eq!(trailers_of(&fixture, "HEAD"), "Refs: #7\nLane-note: first\nLane-note: second", "{message}");
}

#[test]
fn squash_published() {
    let fixture = Fixture::new();
    lane_commit(&fixture, "published lane commit", &[]);
    git(&fixture.repo, &["push", "-q", "origin", "HEAD:refs/heads/shared"]);
    let published = git(&fixture.repo, &["rev-parse", "HEAD"]);
    lane_commit(&fixture, "add a line after the published commit", &[]);
    lane_commit(&fixture, "add another line after the published commit", &[]);
    queue_lane(&fixture, None);
    git(&fixture.repo, &["merge-base", "--is-ancestor", &published, "HEAD"]);
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{published}..HEAD")]), "1", "the unpublished lane commits did not fold to one after the published one");
}

#[test]
fn squash_message() {
    let fixture = Fixture::new();
    lane_commit(&fixture, "outgoing", &["Refs: #9"]);
    let message = fixture.root.path().join("message.txt");
    write(&message, "Name the change once\n\nWhy it lands.\n\nLane-note: supplied\n");
    queue_lane(&fixture, Some(&fs::read_to_string(&message).unwrap()));
    assert_eq!(message_of(&fixture, "HEAD"), "Name the change once\n\nWhy it lands.\n\nLane-note: supplied\nRefs: #9");
}
