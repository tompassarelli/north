use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::candidate::{Pick, build, ensure};
use super::queue::Ticket;

fn run(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Lane", "-c", "user.email=lane@example.invalid"])
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-11-10T16:54:22Z")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, file: &str, text: &str, message: &str, amend: bool) -> String {
    fs::write(repo.join(file), text).unwrap();
    run(repo, &["add", "--all"]);
    run(repo, &["commit", "-q", "-m", message].into_iter().chain(amend.then_some("--amend")).collect::<Vec<_>>());
    run(repo, &["rev-parse", "HEAD"])
}

fn ticket(repo: &Path, dir: &Path, head: &str) -> (PathBuf, Ticket) {
    (dir.join("lane.json"), Ticket { branch: "lane".to_owned(), head: head.to_owned(), root: repo.to_owned(), origin: repo.display().to_string(), ..Default::default() })
}

#[test]
fn a_lane_whose_patch_is_already_on_main_is_not_landed_again() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    commit(&repo, "a", "a\n", "base", false);
    run(&repo, &["checkout", "-q", "-b", "lane"]);
    let landed = commit(&repo, "t", "one\n", "Admit a launch by its measured pressure", false);
    run(&repo, &["checkout", "-q", "main"]);
    run(&repo, &["cherry-pick", &landed]);
    let carrier = run(&repo, &["rev-parse", "HEAD"]);
    run(&repo, &["checkout", "-q", "lane"]);
    let amended = commit(&repo, "u", "two\n", "Admit a launch by its measured pressure", true);
    run(&repo, &["checkout", "-q", "main"]);
    let (worktree, logs) = (dir.path().join("worktrees/lander"), dir.path().join("logs"));
    ensure(&repo, &worktree).unwrap();

    let again = build(&worktree, &carrier, &[ticket(&repo, dir.path(), &landed)], &[], usize::MAX, &logs).unwrap();
    assert!(matches!(&again.picks[0], Pick::Decided(text) if *text == format!("landed {carrier}")), "the lane's own patch is on main");

    let changed = build(&worktree, &carrier, &[ticket(&repo, dir.path(), &amended)], &[], usize::MAX, &logs).unwrap();
    let Pick::Decided(verdict) = &changed.picks[0] else { panic!("an amended copy of a landed commit was picked") };
    assert!(verdict.starts_with("failed "), "{verdict}");
    let log = fs::read_to_string(verdict.trim_start_matches("failed ")).unwrap();
    assert!(log.contains(&format!("was already landed as {carrier}")), "{log}");
    assert_eq!(changed.included(), 0);
}
