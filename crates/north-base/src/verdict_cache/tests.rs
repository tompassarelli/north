use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Check, Pass, Store, key};
use crate::foundation::clock::Timestamp;

const SAFE_PUSH: &str = "[land]\n\tcheck = src/ :: bash bin/check\n\tafter = @rust-input :: cargo clippy\n\tafter = docs/ :: ./bin/head-north docs check\n";
const CHECK: &str = "bash bin/check";

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, text).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn repository() -> (tempfile::TempDir, String) {
    let scratch = tempfile::tempdir().unwrap();
    git(scratch.path(), &["init", "-q", "-b", "main"]);
    let head = commit(scratch.path(), &[(".safe-push", SAFE_PUSH), ("src/lib.rs", "fn a() {}"), ("bin/check", "#!/bin/bash\ncat data/input.txt\n"), ("data/input.txt", "one"), ("docs/a.md", "a"), ("Cargo.toml", "[workspace]")], "base");
    (scratch, head)
}

fn key_with(root: &Path, tree: &str, command: &str, runtime: &str, environment: &[(String, String)]) -> String {
    key(&Check { repo: root, tree, command, runtime, environment, machine_repos: &[] }).unwrap()
}

fn key_of(root: &Path, tree: &str, command: &str) -> String {
    key_with(root, tree, command, "runtime-a", &[])
}

#[test]
fn an_identical_tree_at_a_new_commit_hits_the_pass_recorded_for_it() {
    let (repo, first) = repository();
    let store = tempfile::tempdir().unwrap();
    let store = Store(store.path().to_owned());
    let recorded = key_of(repo.path(), &first, CHECK);
    store.record(&recorded, Some(&Pass { at: Timestamp::from_unix_seconds(1_000), sha: first.clone() })).unwrap();
    let relanded = commit(repo.path(), &[], "the same tree again");
    assert_ne!(relanded, first);
    let again = key_of(repo.path(), &relanded, CHECK);
    assert_eq!(again, recorded);
    assert_eq!(store.hit(&again).map(|pass| pass.sha), Some(first));
}

#[test]
fn a_changed_input_misses_and_an_unread_path_does_not() {
    let (repo, first) = repository();
    let before = key_of(repo.path(), &first, CHECK);
    let docs = commit(repo.path(), &[("docs/a.md", "b")], "docs only");
    assert_eq!(key_of(repo.path(), &docs, CHECK), before);
    let source = commit(repo.path(), &[("src/lib.rs", "fn b() {}")], "selected path");
    assert_ne!(key_of(repo.path(), &source, CHECK), before);
    let read = key_of(repo.path(), &source, CHECK);
    let data = commit(repo.path(), &[("data/input.txt", "two")], "a file the check script reads");
    assert_ne!(key_of(repo.path(), &data, CHECK), read);
    let clippy = key_of(repo.path(), &data, "cargo clippy");
    let manifest = commit(repo.path(), &[("Cargo.toml", "[workspace]\nmembers = []")], "a rust input");
    assert_ne!(key_of(repo.path(), &manifest, "cargo clippy"), clippy);
}

#[test]
fn a_check_that_builds_north_from_the_tree_misses_when_any_rust_input_changes() {
    let (repo, first) = repository();
    let command = "./bin/head-north docs check";
    let before = key_of(repo.path(), &first, command);
    let unread = commit(repo.path(), &[("data/input.txt", "two")], "a file it does not read");
    assert_eq!(key_of(repo.path(), &unread, command), before);
    let source = commit(repo.path(), &[("src/lib.rs", "fn c() {}")], "a crate it builds");
    assert_ne!(key_of(repo.path(), &source, command), before);
}

#[test]
fn the_runtime_and_the_check_environment_are_part_of_the_key() {
    let (repo, head) = repository();
    let narrow = [("SAFE_PUSH_LAND_PACKAGES".to_owned(), "-p a".to_owned())];
    let wide = [("SAFE_PUSH_LAND_PACKAGES".to_owned(), "--workspace".to_owned())];
    let base = key_with(repo.path(), &head, CHECK, "runtime-a", &narrow);
    assert_ne!(key_with(repo.path(), &head, CHECK, "runtime-b", &narrow), base);
    assert_ne!(key_with(repo.path(), &head, CHECK, "runtime-a", &wide), base);
    let same_tree = commit(repo.path(), &[], "empty");
    let at = |revision: &str| [("SAFE_PUSH_LAND_BASE".to_owned(), revision.to_owned())];
    assert_eq!(key_with(repo.path(), &head, CHECK, "runtime-a", &at(&head)), key_with(repo.path(), &head, CHECK, "runtime-a", &at(&same_tree)));
    let other = commit(repo.path(), &[("docs/a.md", "c")], "another base");
    assert_eq!(key_with(repo.path(), &head, "cargo clippy", "runtime-a", &at(&head)), key_with(repo.path(), &head, "cargo clippy", "runtime-a", &at(&other)));
    let north = "./bin/head-north docs check";
    assert_ne!(key_with(repo.path(), &head, north, "runtime-a", &at(&head)), key_with(repo.path(), &head, north, "runtime-a", &at(&other)));
}

#[test]
fn a_failure_removes_the_pass_and_is_never_stored() {
    let (repo, head) = repository();
    let store = tempfile::tempdir().unwrap();
    let store = Store(store.path().to_owned());
    let recorded = key_of(repo.path(), &head, CHECK);
    store.record(&recorded, Some(&Pass { at: Timestamp::from_unix_seconds(1_000), sha: head })).unwrap();
    store.record(&recorded, None).unwrap();
    assert!(store.hit(&recorded).is_none());
    store.record(&recorded, None).unwrap();
    assert!(store.hit(&recorded).is_none());
    assert_eq!(fs::read_dir(&store.0).unwrap().count(), 0);
}
