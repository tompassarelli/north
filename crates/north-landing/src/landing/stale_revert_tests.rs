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
fn a_lane_based_before_a_commit_that_rewrites_its_lines_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x", START + HOUR);
    commit_at(root, "other.txt", "unrelated\n", "main moves on", START + 72 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = commit_at(root, "app.txt", "alpha\nbeta from lane\ngamma\n", "lane", START + 2 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    commit_at(root, "app.txt", "alpha\nbeta from lane\ngamma\n", "lane rebased with its side taken", START + 73 * HOUR);
    let found = reverted(root, "main", "landed", &lane).unwrap();
    assert_eq!(shas(&found), [x.as_str()]);
    assert_eq!(found[0].files.keys().collect::<Vec<_>>(), ["app.txt"]);
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


fn remove_at(root: &Path, path: &str, message: &str, at: i64) -> String {
    std::fs::remove_file(root.join(path)).unwrap();
    commit_staged(root, message, at)
}

#[test]
fn a_stale_squash_that_deletes_a_file_main_added_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    let x = commit_at(root, "port.txt", "one\ntwo\nthree\nfour\n", "x adds a file", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = commit_at(root, "notes.txt", "lane work\n", "lane", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    remove_at(root, "port.txt", "squash that never saw the file", START + 102 * HOUR);
    let found = reverted(root, "main", "landed", &lane).unwrap();
    assert_eq!(shas(&found), [x.as_str()]);
    assert_eq!(found[0].files.keys().collect::<Vec<_>>(), ["port.txt"]);
}

