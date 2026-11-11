use std::path::Path;

use super::*;

const HOLDS: &str = "[[hold]]\nid = \"H3\"\nwhat = \"A deliberate stop never rolls a runtime back\"\nguard_kind = \"test\"\nguard = \"an_ordered_stop_is_not_a_runtime_exit\"\nstate = \"live\"\n";
const GUARD: &str = "#[test]\nfn an_ordered_stop_is_not_a_runtime_exit() {}\n";
const TESTS: &str = "crates/runtime/src/tests.rs";

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
fn a_lane_that_deletes_a_holdlist_guard_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, &[(HOLDLIST, HOLDS), (TESTS, GUARD)], "base");

    let absent = commit(repo, &[(TESTS, "fn helper() {}\n")], "drop the test");
    let refusal = check(repo, &base, &absent).unwrap_err();
    assert!(refusal.contains("H3: test `an_ordered_stop_is_not_a_runtime_exit` is live in harness/holdlist.toml but no `fn an_ordered_stop_is_not_a_runtime_exit(` is in the lane's Rust files"), "{refusal}");

    let present = commit(repo, &[(TESTS, GUARD)], "write it back");
    assert_eq!(check(repo, &base, &present), Ok(()));
}
