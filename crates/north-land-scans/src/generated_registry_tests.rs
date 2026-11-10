use std::path::Path;

use super::*;

fn git_in(directory: &Path, arguments: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?}");
}

const CONFIG: &str = "[[set]]\npath = \"docs/features.md\"\nkind = \"regenerate\"\ncommand = \"north docs features-index\"\nsources = [\"docs/features/\"]\ngenerator = [\"crates/north-ops/src/ops/docs.rs\"]\n";

fn commit(directory: &Path, message: &str) -> String {
    git_in(directory, &["add", "-A"]);
    git_in(directory, &["commit", "-qm", message]);
    git::text(directory, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn repo() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path();
    git_in(directory, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(directory.join("harness")).unwrap();
    std::fs::create_dir_all(directory.join("docs/features/01-landing")).unwrap();
    std::fs::write(directory.join("harness/merge-sets.toml"), CONFIG).unwrap();
    std::fs::write(directory.join("docs/features/01-landing/0001-enqueue.md"), "- Enqueue is cheap.\n").unwrap();
    std::fs::write(directory.join("docs/features.md"), "- Enqueue is cheap.\n").unwrap();
    root
}

#[test]
fn a_lane_that_commits_the_generated_features_index_is_refused_and_names_the_source_file_to_edit() {
    let root = repo();
    let directory = root.path();
    let base = commit(directory, "base");
    std::fs::write(directory.join("docs/features/01-landing/0001-enqueue.md"), "- Enqueue is cheap and early.\n").unwrap();
    std::fs::write(directory.join("docs/features.md"), "- Enqueue is cheap and early.\n").unwrap();
    let head = commit(directory, "lane");

    let error = check(directory, &base, &head).unwrap_err();
    assert!(error.contains("docs/features.md is generated from docs/features/"), "{error}");
    assert!(error.contains("edit docs/features/01-landing/0001-enqueue.md instead"), "{error}");
}

#[test]
fn a_lane_that_edits_only_the_per_unit_source_is_accepted() {
    let root = repo();
    let directory = root.path();
    let base = commit(directory, "base");
    std::fs::write(directory.join("docs/features/01-landing/0001-enqueue.md"), "- Enqueue is cheap and early.\n").unwrap();
    let head = commit(directory, "lane");

    assert_eq!(check(directory, &base, &head), Ok(()));
}
