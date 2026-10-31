use std::fs;
use std::path::Path;
use std::process::Command;

fn run(repo: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo)
        .env("XDG_STATE_HOME", repo.join("../state"))
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn land(repo: &Path, files: &[(&str, String)]) {
    for (path, text) in files {
        fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        fs::write(repo.join(path), text).unwrap();
    }
    run(repo, "git", &["add", "."]);
    run(repo, "git", &["commit", "-qm", "land"]);
}

#[test]
fn a_moved_file_counts_only_the_lines_its_move_changed_so_a_crate_split_does_not_read_as_new_code() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/code-health/cli.ts");
    let cli = cli.to_str().unwrap();
    let signal = || run(&repo, "bun", &[cli, "signal", "--repo", repo.to_str().unwrap()]);
    let source = "export const x = 1;\n".repeat(40);
    run(&repo, "git", &["init", "-q"]);
    land(&repo, &[
        ("structure.toml", "[review]\ncommits = 0\nhand_lines_added = 5\ntest_lines_changed = 5\ndays = 0\n".to_owned()),
        ("src/big.ts", source.clone()),
        ("tests/big.test.ts", "t\n".repeat(40)),
    ]);
    run(&repo, "bun", &[cli, "review-done", "--repo", repo.to_str().unwrap()]);
    run(&repo, "git", &["mv", "src", "crate"]);
    run(&repo, "git", &["mv", "tests", "crate-tests"]);
    land(&repo, &[("crate/big.ts", format!("{source}export const y = 2;\n"))]);
    assert_eq!(signal(), "quiet");
    land(&repo, &[("crate/new.ts", "export const n = 1;\n".repeat(5))]);
    assert_eq!(signal(), "FIRE 6 hand lines added since review (limit 5)");
}
