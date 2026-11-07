use super::comments::added;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn an_added_plain_comment_is_reported_and_an_added_constraint_line_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(
        repo.join("lib.rs"),
        "// old note kept as it was\nfn a() {}\n",
    )
    .unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "base"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    let text = "// old note kept as it was\nfn a() {}\n// bump the counter\nfn b() {\n    let url = \"http://not-a-comment\";\n}\n// Lua 5.3 has no integer division here, so keep the float path\nfn c() {}\n/// documented\nfn d() {}\n";
    std::fs::write(repo.join("lib.rs"), text).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "head"]);
    let head = git(repo, &["rev-parse", "HEAD"]);
    let found = added(repo, &base, &head).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("code-comment lib.rs:3 "), "{found:?}");
    assert!(
        found[0].contains("delete it, or make it one line that names"),
        "{found:?}"
    );
}
