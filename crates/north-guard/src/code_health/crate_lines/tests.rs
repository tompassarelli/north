use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(repo).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit(repo: &Path, lines: usize, message: &str) {
    std::fs::write(repo.join("src/lib.rs"), "pub fn f() {}\n".repeat(lines)).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", message]);
}

#[test]
fn a_crate_over_its_budget_may_grow_only_when_the_landing_records_why() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"solo\"\n").unwrap();
    std::fs::write(repo.join("structure.toml"), "[crate_lines]\ndefault = 1\n").unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    commit(repo, 2, "base");
    commit(repo, 3, "grow");
    let (lines, code) = super::gate(repo, "HEAD~1", "HEAD").unwrap();
    assert_eq!(code, 1, "{lines:?}");
    assert!(lines[0].contains("solo grew 2 -> 3") && lines[0].contains("Code-health-allow: crate-lines solo"), "{lines:?}");
    commit(repo, 4, "grow again\n\nCode-health-allow: crate-lines solo — the parser moves out next");
    assert_eq!(super::gate(repo, "HEAD~1", "HEAD").unwrap(), (vec!["crate-lines: no crate over its cap grew".to_owned()], 0));
}
