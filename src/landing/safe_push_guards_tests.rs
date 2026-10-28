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

fn shell_function(name: &str) -> String {
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push")).unwrap();
    let start = format!("{name}() {{");
    let body: Vec<&str> = script.lines().skip_while(|line| *line != start).collect();
    let end = body.iter().position(|line| *line == "}").unwrap_or_else(|| panic!("bin/safe-push lost {name}"));
    body[..=end].iter().map(|line| format!("{line}\n")).collect()
}

fn shell_scan(repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    let range = format!("{}..HEAD", repository.base);
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{}commit_email_scan \"$@\"\n", shell_function("commit_email_scan")))
        .arg("safe-push")
        .arg(&range)
        .current_dir(&repository.dir)
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_EMAIL")
        .envs(pushing.iter().copied())
        .output()
        .unwrap();
    (output.status.code().unwrap(), String::from_utf8(output.stdout).unwrap())
}

fn rust_scan(repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    let range = format!("{}..HEAD", repository.base);
    let emails: Vec<String> = pushing.iter().map(|(_, email)| (*email).to_owned()).collect();
    match outgoing_email_refusal(&repository.dir, &[range], &emails) {
        Some(message) => (1, format!("{message}\n")),
        None => (0, String::new()),
    }
}

fn same_verdict(case: &str, repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    let shell = shell_scan(repository, pushing);
    assert_eq!(shell, rust_scan(repository, pushing), "{case}: the shell and Rust email scans disagree");
    shell
}

fn refused(case: &str, repository: &Repository, pushing: &[(&str, &str)], role: &str, commit: &str) {
    let (status, stdout) = same_verdict(case, repository, pushing);
    let short = git(&repository.dir, &["rev-parse", "--short", commit], &[]);
    assert_eq!(status, 1, "{case}: {stdout}");
    assert!(stdout.starts_with(&format!("safe-push: commit {short} {role} email {PRIVATE} is not {PUSHER}")), "{case}: {stdout}");
}

#[test]
fn safe_push_email_scan_refuses_private_authors_and_committers_like_the_shell() {
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

#[test]
fn safe_push_email_scan_passes_the_pusher_and_allowed_emails_like_the_shell() {
    let allowed = repository();
    git(&allowed.dir, &["config", "--add", "safe-push.allowEmail", "bot@allowed.example"], &[]);
    git(&allowed.dir, &["config", "--add", "safe-push.allowEmail", "Release@Allowed.Example"], &[]);
    empty_commit(&allowed.dir, "pusher email in another case", &[("GIT_AUTHOR_EMAIL", "Safe-Push-Test@Example.Invalid")]);
    empty_commit(&allowed.dir, "allowed bot", &[("GIT_AUTHOR_EMAIL", "bot@allowed.example"), ("GIT_COMMITTER_EMAIL", "release@allowed.example")]);
    git(&allowed.dir, &["commit", "--allow-empty", "-qm", "co-authored", "-m", &format!("Co-Authored-By: someone <{PRIVATE}>")], &[]);
    assert_eq!(same_verdict("allowEmail", &allowed, &[]), (0, String::new()));

    let identity = [("GIT_AUTHOR_EMAIL", "ci@env.example"), ("GIT_COMMITTER_EMAIL", "ci@env.example")];
    let pushing = repository();
    empty_commit(&pushing.dir, "env identity", &identity);
    assert_eq!(same_verdict("the pushing identity", &pushing, &identity), (0, String::new()));

    let nothing = repository();
    assert_eq!(same_verdict("no outgoing commits", &nothing, &[]), (0, String::new()));
}
