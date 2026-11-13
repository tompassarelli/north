use super::*;

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
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

fn commit(repo: &Path, text: &str) -> String {
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/lib.rs"), text).unwrap();
    git_in(repo, &["add", "src/lib.rs"]);
    git_in(repo, &["commit", "-qm", "c"]);
    git_in(repo, &["rev-parse", "HEAD"])
}

const BASE: &str = "pub fn double(x: u32) -> u32 {\n    x + x\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn doubles() {\n        let text = \"}\";\n        assert_eq!(double(2), 4);\n    }\n}\n";

#[test]
fn an_assertion_added_inside_an_existing_in_file_test_counts_as_a_test_change_while_a_fix_outside_it_does_not_north_1329() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, BASE);

    let inside = commit(repo, &BASE.replace("        assert_eq!(double(2), 4);\n", "        assert_eq!(double(2), 4);\n        assert_eq!(double(3), 6);\n"));
    assert!(needs_no_test(repo, &base, &inside));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let outside = commit(repo, &BASE.replace("    x + x\n", "    x * 2\n"));
    assert!(!needs_no_test(repo, &base, &outside));
}
