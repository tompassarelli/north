use std::path::Path;

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

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path) -> String {
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-qm", "step"]);
    git_in(root, &["rev-parse", "HEAD"])
}

#[test]
fn the_north_ops_split_adding_north_flow_without_a_crate_dag_entry_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    write(repo, "structure.toml", "[crate_dag]\n\n[[crate_dag.crate]]\nname = \"north-base\"\npaths = [\"crates/north-base/src/**\"]\nmay_use = []\n");
    write(repo, "crates/north-base/Cargo.toml", "[package]\nname = \"north-base\"\n");
    write(repo, "crates/north-base/src/lib.rs", "pub fn base() {}\n");
    let base = commit(repo);

    write(repo, "crates/north-flow/Cargo.toml", "[package]\nname = \"north-flow\"\n");
    write(repo, "crates/north-flow/src/lib.rs", "pub fn flow() {}\n");
    let split = commit(repo);

    let (lines, code) = super::verdict(repo, &base, &split).unwrap();
    assert_eq!((code, lines.iter().any(|line| line.contains("adds crate north-flow with no [[crate_dag.crate]] entry"))), (1, true), "{lines:?}");
}

#[test]
fn a_crate_covered_only_through_module_path_patterns_is_not_reported_as_unplanned() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    write(repo, "structure.toml", "[crate_dag]\n\n[[crate_dag.crate]]\nname = \"north-capacity\"\npaths = [\"crates/*/src/capacity.rs\", \"crates/*/src/capacity/**\"]\nmay_use = []\n");
    let empty = commit(repo);
    write(repo, "crates/north-capacity/Cargo.toml", "[package]\nname = \"north-capacity\"\n");
    write(repo, "crates/north-capacity/src/lib.rs", "pub mod capacity;\n");
    write(repo, "crates/north-capacity/src/capacity.rs", "pub fn capacity() {}\n");
    let added = commit(repo);

    let (lines, code) = super::verdict(repo, &empty, &added).unwrap();
    assert_eq!(code, 0, "{lines:?}");
}
