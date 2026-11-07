use super::*;

const HOUR: i64 = 60 * 60;
const START: i64 = 1_790_000_000;

fn git_ok(root: &Path, args: &[&str]) -> String {
    git(root, args).unwrap_or_else(|error| panic!("{error}")).trim().to_owned()
}

fn commit_staged(root: &Path, message: &str, at: i64) -> String {
    git_ok(root, &["add", "-A"]);
    let date = format!("@{at} +0000");
    let status = Command::new("git")
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args(["-c", "commit.gpgsign=false", "commit", "-qm", message])
        .status()
        .unwrap();
    assert!(status.success());
    git_ok(root, &["rev-parse", "HEAD"])
}

fn commit_at(root: &Path, path: &str, text: &str, message: &str, at: i64) -> String {
    std::fs::write(root.join(path), text).unwrap();
    commit_staged(root, message, at)
}

fn fixture() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.name", "fixture"]);
    git_ok(root, &["config", "user.email", "fixture@example.invalid"]);
    commit_at(root, "app.txt", "alpha\nbeta\ngamma\n", "base", START);
    scratch
}

fn shas(found: &[Reverted]) -> Vec<&str> {
    found.iter().map(|reverted| reverted.commit.as_str()).collect()
}

#[test]
fn a_soft_reset_squash_onto_moved_main_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "notes.txt", "lane work\n", "lane", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "main"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    git_ok(root, &["reset", "-q", "--soft", "main"]);
    commit_at(root, "notes.txt", "lane work squashed\n", "lane squashed without a rebase", START + 102 * HOUR);
    assert_eq!(shas(&reverted(root, "main", "lane", "lane").unwrap()), [x.as_str()]);
}


#[test]
fn moving_a_recent_function_into_a_new_file_with_its_locals_as_fields_is_no_revert() {
    let scratch = fixture();
    let root = scratch.path();
    let empty = "fn serve() {\n    loop {\n    }\n}\n";
    commit_at(root, "serve.rs", empty, "serve loop", START + 100 * HOUR);
    let body = "fn serve() {\n    loop {\n        if !red_only && let Some(reason) = pause::held(&self.queue) {\n            red_only = true;\n        }\n        bisect::decide(path, ticket, refusal, Some(&log), &base)?;\n    }\n}\n";
    commit_at(root, "serve.rs", body, "pause holds the loop", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "serve.rs", empty, "move the loop out", START + 102 * HOUR);
    let moved = "fn gate(pass: &mut Pass) {\n    if !pass.red_only && let Some(reason) = pause::held(&self.queue) {\n        pass.red_only = true;\n    }\n    bisect::decide(path, ticket, refusal, Some(&checked.log), attempt.base)?;\n}\n";
    commit_at(root, "batch.rs", moved, "the loop as phases", START + 103 * HOUR);
    assert_eq!(shas(&reverted(root, "main", "lane", "lane").unwrap()), Vec::<&str>::new());
}
