use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use super::refuse_static_gates;

fn run(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "commit.gpgsign=false", "-c", "user.name=Gate fixture", "-c", "user.email=fixture@example.invalid"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn allow(repo: &Path, globs: &[&str]) {
    let entries: Vec<_> = globs.iter().map(|glob| json!({"glob":glob,"class":"script","gap":"fixture gap","provenance":"Tom 2026-10-15: permit this fixture","port_owner":"fixture-port-owner","port_target":"src/ported.rs","commitment":"Rust port by 2026-10-16 18:00","created":"2026-10-15"})).collect();
    fs::write(repo.join(".language-allowlist.json"), json!({"ratchet":["script"],"entries":entries}).to_string()).unwrap();
}

fn commit(repo: &Path, message: &str) -> String {
    run(repo, &["add", "--all"]);
    run(repo, &["commit", "-q", "-m", message]);
    run(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_lane_only_behind_main_is_judged_on_its_own_changes_not_on_what_main_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    run(repo, &["init", "-q", "-b", "main"]);
    allow(repo, &["tool.sh"]);
    let fork = commit(repo, "allow a script");
    allow(repo, &[]);
    let main = commit(repo, "main drops the allowance");
    run(repo, &["checkout", "-q", "-b", "lane", &fork]);
    fs::write(repo.join("notes.md"), "lane change\n").unwrap();
    commit(repo, "lane touches only notes");
    assert!(!crate::language_gate::check(repo, &main, "HEAD"), "diffing from main's tip shows main's removal as the lane widening the allowlist");
    assert_eq!(refuse_static_gates(repo, &main, "HEAD"), Ok(()));
}

struct Lane {
    dir: tempfile::TempDir,
    repo: std::path::PathBuf,
    base: String,
}

impl Lane {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (origin, repo) = (dir.path().join("origin.git"), dir.path().join("lane"));
        run(dir.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        run(dir.path(), &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        run(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(repo.join("content.txt"), "base\n").unwrap();
        let base = commit(&repo, "base");
        run(&repo, &["push", "-q", "origin", "main"]);
        run(&repo, &["switch", "-q", "-c", "lane"]);
        run(&repo, &["commit", "-q", "--allow-empty", "-m", "outgoing"]);
        Self { dir, repo, base }
    }

    fn commit(&self, subject: &str, trailers: &[&str]) {
        fs::write(self.repo.join("content.txt"), format!("{}{subject}\n", fs::read_to_string(self.repo.join("content.txt")).unwrap())).unwrap();
        run(&self.repo, &["add", "--all"]);
        let mut args = vec!["commit", "-q", "-m", subject];
        for trailer in trailers {
            args.extend(["--trailer", trailer]);
        }
        run(&self.repo, &args);
    }

    fn enqueue(&self, shape: super::Shape) -> super::Queued {
        let (state, south) = (self.dir.path().join("state"), self.dir.path().join("absent-south"));
        let environment = super::Environment { state_home: &state, host: "fixture-host", session: None, south: &south, denylist: None };
        super::enqueue(&self.repo, shape, false, &environment).unwrap()
    }
}

#[test]
fn a_queued_lane_is_one_commit_keeping_each_trailer_once_unless_several_commits_keeps_it_as_it_is() {
    let lane = Lane::new();
    lane.commit("add the first lane line", &["Refs: #7", "Lane-note: first"]);
    lane.commit("add the second lane line", &["Refs: #7", "Lane-note: second"]);
    let tree = run(&lane.repo, &["rev-parse", "HEAD^{tree}"]);
    let queued = lane.enqueue(super::Shape::OneCommit { message: None });
    assert_eq!((queued.ticket.head, run(&lane.repo, &["rev-parse", "HEAD^"]), run(&lane.repo, &["rev-parse", "HEAD^{tree}"])), (run(&lane.repo, &["rev-parse", "HEAD"]), lane.base.clone(), tree));
    let message = run(&lane.repo, &["show", "-s", "--format=%B", "HEAD"]);
    assert!(message.starts_with("outgoing\n") && message.lines().any(|line| line == "- add the second lane line"), "{message}");
    assert_eq!(run(&lane.repo, &["show", "-s", "--format=%(trailers:only,unfold)", "HEAD"]), "Refs: #7\nLane-note: first\nLane-note: second");

    let lane = Lane::new();
    run(&lane.repo, &["push", "-q", "origin", "HEAD:refs/heads/shared"]);
    let published = run(&lane.repo, &["rev-parse", "HEAD"]);
    lane.commit("add a line after the published commit", &[]);
    lane.commit("add another line after the published commit", &[]);
    lane.enqueue(super::Shape::OneCommit { message: None });
    assert_eq!(run(&lane.repo, &["rev-parse", "HEAD^"]), published, "squashing rewrote a commit already on origin");

    let lane = Lane::new();
    run(&lane.repo, &["commit", "--amend", "--allow-empty", "-q", "-m", "outgoing", "--trailer", "Refs: #9"]);
    lane.enqueue(super::Shape::OneCommit { message: Some("Name the change once\n\nWhy it lands.\n\nLane-note: supplied\n") });
    assert_eq!(run(&lane.repo, &["show", "-s", "--format=%B", "HEAD"]), "Name the change once\n\nWhy it lands.\n\nLane-note: supplied\nRefs: #9");

    let lane = Lane::new();
    lane.commit("a line kept as its own commit", &[]);
    let head = run(&lane.repo, &["rev-parse", "HEAD"]);
    let queued = lane.enqueue(super::Shape::SeveralCommits { reason: "each commit is a bisect point" });
    assert_eq!((queued.ticket.several_commits.as_deref(), queued.ticket.head.as_str()), (Some("each commit is a bisect point"), head.as_str()));
    assert_eq!(run(&lane.repo, &["rev-list", "--count", "origin/main..HEAD"]), "2", "--several-commits squashed the lane");
}
