use std::path::Path;

use super::*;
use north_base::platform::review_accept::{Accept, patch_id};

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", message]);
    git_in(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_lane_that_changes_a_safe_push_key_lands_only_with_a_review_accept_that_a_commit_message_cannot_grant_north_1287() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(SETTINGS, "[land]\n\tcheck = . :: cargo test\n\tcheck = . :: cargo clippy\n[landing]\n\tqueue = lander\n"), ("src/a.rs", "a\n")], "base");

    let outside = commit(&repo, &[("src/a.rs", "b\n")], "outside .safe-push");
    assert_eq!(check(&repo, &base, &outside, &accepts, &[]), Ok(()), "a lane that leaves .safe-push alone needs no pass");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let removed = commit(&repo, &[(SETTINGS, "[land]\n\tcheck = . :: cargo test\n[landing]\n\tqueue = lander\n")], "drop clippy\n\nreviewed by reviewer-a");
    let refusal = check(&repo, &base, &removed, &accepts, &[]).unwrap_err();
    assert!(refusal.contains("land.check"), "{refusal}");
    assert!(!refusal.contains("landing.queue"), "{refusal}");

    let fork = git::merge_base(&repo, &base, &removed);
    let patch = patch_id(&repo, &fork, &removed).unwrap();
    let record = |session: &str| accepts.record(&Accept { patch_id: patch.clone(), session: session.to_owned(), at: 1 }).unwrap();
    assert!(check(&repo, &base, &removed, &accepts, &[]).is_err(), "a reviewer named in the message grants nothing");

    record("author-x");
    let author = Finished { session: "author-x".to_owned(), command: String::new(), head: removed.clone(), passed: true, at: 1 };
    assert!(check(&repo, &base, &removed, &accepts, std::slice::from_ref(&author)).is_err(), "the lane's author is not its reviewer");

    record("reviewer-a");
    assert_eq!(check(&repo, &base, &removed, &accepts, std::slice::from_ref(&author)), Ok(()), "a recorded reviewer pass admits the lane");
}
