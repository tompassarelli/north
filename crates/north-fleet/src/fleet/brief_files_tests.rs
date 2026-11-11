use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).current_dir(dir).output().expect("git");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn the_brief_carries_the_base_content_of_the_tracked_files_it_names_and_says_which_it_leaves_out() {
    let top = tempfile::tempdir().expect("tempdir");
    let repo = top.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("src");
    std::fs::write(top.path().join("outside.rs"), "outside the repository").expect("outside");
    std::fs::write(repo.join("src/lib.rs"), "pub fn named() {}\n").expect("lib");
    std::fs::write(repo.join("src/big.rs"), "x".repeat(9000)).expect("big");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "src/lib.rs", "src/big.rs"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    let blob = git(&repo, &["rev-parse", "HEAD:src/lib.rs"]);
    std::fs::write(repo.join("src/lib.rs"), "pub fn edited_after_the_base() {}\n").expect("edit");
    std::fs::write(repo.join("src/new.rs"), "untracked").expect("new");
    let brief = format!("Files: `src/lib.rs`, {}/src/big.rs, src/new.rs and ../outside.rs, {}.", repo.display(), top.path().join("outside.rs").display());
    let shown = shown(&brief, &repo, None);
    assert!(shown.contains(&format!("=== src/lib.rs (blob {blob})\npub fn named() {{}}")), "{shown}");
    assert!(shown.contains("Left out: src/big.rs is over 8 KB (9000 bytes)"), "{shown}");
    assert!(shown.contains("Left out: src/new.rs is not tracked at"), "{shown}");
    assert!(!shown.contains("edited_after_the_base") && !shown.contains("=== src/new.rs") && !shown.contains("outside the repository") && !shown.contains("Left out: ../outside.rs"), "{shown}");
}
