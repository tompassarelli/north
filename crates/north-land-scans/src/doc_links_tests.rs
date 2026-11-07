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

fn commit(directory: &Path) -> String {
    git_in(directory, &["add", "-A"]);
    git_in(directory, &["commit", "-qm", "step"]);
    git::text(directory, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

#[test]
fn the_north_ops_split_that_moved_the_arms_report_without_its_feedback_loop_cite_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let report = repo.join("crates/north-ops/src/ops/report");
    std::fs::create_dir_all(&report).unwrap();
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::write(report.join("arms.rs"), "pub fn arms() {}\n").unwrap();
    let doc = "| signal | writer | reader on main | state | lane |\n|---|---|---|---|---|\n| runs.arm | x | crates/north-ops/src/ops/report/arms.rs, Wilson 95% per arm | read | |\n";
    std::fs::write(repo.join(cites::FEEDBACK_LOOP), doc).unwrap();
    let base = commit(repo);

    std::fs::create_dir_all(repo.join("crates/north-climb/src")).unwrap();
    git_in(repo, &["mv", "crates/north-ops/src/ops/report/arms.rs", "crates/north-climb/src/arms.rs"]);
    let split = commit(repo);

    let mut out = Vec::new();
    let status = scan(repo, &format!("{base}..{split}"), &mut out);
    let text = String::from_utf8(out).unwrap();
    assert_eq!((status, text.contains("runs.arm: crates/north-ops/src/ops/report/arms.rs does not exist")), (1, true), "{text}");
}
