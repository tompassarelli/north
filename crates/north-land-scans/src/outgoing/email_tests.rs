use std::path::Path;
use std::process::Command;

use super::*;

const PUSHER: &str = "safe-push-test@example.invalid";
const PRIVATE: &str = "someone@private.example";

fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_EMAIL")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

fn empty_commit(dir: &Path, message: &str, env: &[(&str, &str)]) {
    git(dir, &["commit", "--allow-empty", "-qm", message], env);
}

struct Repository {
    _scratch: tempfile::TempDir,
    dir: std::path::PathBuf,
    base: String,
}

fn repository() -> Repository {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"], &[]);
    git(&dir, &["config", "user.name", "safe-push-test"], &[]);
    git(&dir, &["config", "user.email", PUSHER], &[]);
    std::fs::write(dir.join("content.txt"), "initial\n").unwrap();
    git(&dir, &["add", "content.txt"], &[]);
    git(&dir, &["commit", "-qm", "initial"], &[]);
    let base = git(&dir, &["rev-parse", "HEAD"], &[]);
    Repository { _scratch: scratch, dir, base }
}

fn rust_scan(repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    let range = format!("{}..HEAD", repository.base);
    let emails: Vec<String> = pushing.iter().map(|(_, email)| (*email).to_owned()).collect();
    match outgoing_email_refusal(&repository.dir, &[range], &emails) {
        Some(message) => (1, format!("{message}\n")),
        None => (0, String::new()),
    }
}

fn same_verdict(_case: &str, repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    rust_scan(repository, pushing)
}

fn refused(case: &str, repository: &Repository, pushing: &[(&str, &str)], role: &str, commit: &str) {
    let (status, stdout) = same_verdict(case, repository, pushing);
    let short = git(&repository.dir, &["rev-parse", "--short", commit], &[]);
    assert_eq!(status, 1, "{case}: {stdout}");
    assert!(stdout.starts_with(&format!("safe-push: commit {short} {role} email {PRIVATE} is not {PUSHER}")), "{case}: {stdout}");
}

#[test]
fn safe_push_email_scan_refuses_private_authors_and_committers() {
    let author = repository();
    empty_commit(&author.dir, "private author", &[("GIT_AUTHOR_EMAIL", PRIVATE)]);
    empty_commit(&author.dir, "later clean commit", &[]);
    refused("author", &author, &[], "author", "HEAD~1");

    let committer = repository();
    empty_commit(&committer.dir, "private committer", &[("GIT_COMMITTER_EMAIL", PRIVATE)]);
    refused("committer", &committer, &[], "committer", "HEAD");

    let applied = repository();
    std::fs::write(applied.dir.join("content.txt"), "cloud work\n").unwrap();
    git(&applied.dir, &["commit", "-qam", "cloud patch"], &[("GIT_AUTHOR_NAME", "someone"), ("GIT_AUTHOR_EMAIL", PRIVATE)]);
    let patches = applied.dir.with_file_name("patches");
    git(&applied.dir, &["format-patch", "-q", "-1", "-o", &patches.to_string_lossy()], &[]);
    git(&applied.dir, &["reset", "-q", "--hard", "HEAD~1"], &[]);
    let patch = std::fs::read_dir(&patches).unwrap().next().unwrap().unwrap().path();
    git(&applied.dir, &["am", "-q", &patch.to_string_lossy()], &[]);
    refused("git am", &applied, &[], "author", "HEAD");

    let other_identity = repository();
    empty_commit(&other_identity.dir, "env identity", &[("GIT_AUTHOR_EMAIL", PRIVATE), ("GIT_COMMITTER_EMAIL", PRIVATE)]);
    refused("another pusher's identity", &other_identity, &[], "author", "HEAD");

    let unset = repository();
    git(&unset.dir, &["config", "user.email", ""], &[]);
    let (status, stdout) = same_verdict("unset user.email", &unset, &[]);
    assert_eq!(status, 1);
    assert!(stdout.starts_with("safe-push: ✗ git config user.email is empty"), "{stdout}");
}
