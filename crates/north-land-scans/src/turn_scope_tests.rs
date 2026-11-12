use std::path::Path;

use super::*;

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

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", "c"]);
    git_in(repo, &["rev-parse", "HEAD"])
}

const ONE: &str = "[[turn]]\ndate = 2026-11-12\n";
const TWO: &str = "[[turn]]\ndate = 2026-11-12\n\n[[turn]]\ndate = 2026-11-13\n";
const BARS: &str = "[[experiment]]\nbar = 1\n";
const MORE_BARS: &str = "[[experiment]]\nbar = 1\n\n[[experiment]]\nbar = 2\n";

#[test]
fn a_lane_outside_a_turns_scope_is_recorded_as_a_would_be_refusal_and_one_inside_it_is_not() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, &[(TURNS, ONE), (EXPERIMENTS, BARS), ("src/a.rs", "a\n")]);

    let inside = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, MORE_BARS)]);
    assert_eq!(check(repo, &base, &inside), Ok(()));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let code = commit(repo, &[(TURNS, TWO), ("src/a.rs", "b\n")]);
    assert!(check(repo, &base, &code).unwrap_err().contains("also changes src/a.rs"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let moved = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, "[[experiment]]\nbar = 9\n")]);
    assert!(check(repo, &base, &moved).unwrap_err().contains("so a bar, window or review day may move"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let rewritten = commit(repo, &[(TURNS, "[[turn]]\ndate = 2026-11-11\n")]);
    assert!(check(repo, &base, &rewritten).unwrap_err().contains("changes or removes a recorded [[turn]]"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let other = commit(repo, &[("src/a.rs", "c\n")]);
    assert_eq!(check(repo, &base, &other), Ok(()));
    let state = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    assert_eq!(scan(repo, &format!("{base}..{code}"), state.path(), &mut out), 0);
    assert!(String::from_utf8(out).unwrap().contains("would refuse (shadow)"));
    let log = std::fs::read_to_string(state.path().join("north/gates/turn-scope.jsonl")).unwrap();
    assert!(log.contains("\"would_refuse\":true"), "{log}");
}
