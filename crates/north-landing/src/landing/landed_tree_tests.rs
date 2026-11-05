use super::*;

fn git_ok(root: &Path, args: &[&str]) -> String {
    git(root, args).unwrap_or_else(|| panic!("git {args:?} failed"))
}

fn commit(root: &Path, path: &str, text: &str, message: &str) {
    std::fs::write(root.join(path), text).unwrap();
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["-c", "commit.gpgsign=false", "commit", "-qm", message]);
}

fn fixture() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.name", "fixture"]);
    git_ok(root, &["config", "user.email", "fixture@example.invalid"]);
    commit(root, "app.txt", "alpha\nbeta\ngamma\n", "base");
    git_ok(root, &["branch", "lane"]);
    scratch
}

fn lane_edit(root: &Path, text: &str) {
    git_ok(root, &["checkout", "-q", "lane"]);
    commit(root, "app.txt", text, "lane change");
    git_ok(root, &["checkout", "-q", "main"]);
}

#[test]
fn lane_with_work_main_never_held_is_unlanded() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta\ngamma\ndelta\n");
    commit(root, "app.txt", "alpha\nbeta two\ngamma\n", "lane change");
    assert_eq!(landed(root, "main", "lane"), None);
}

#[test]
fn lane_whose_deletion_main_kept_is_unlanded() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\ngamma\n");
    commit(root, "app.txt", "alpha\nbeta\ngamma\nzeta\n", "lane change");
    assert_eq!(landed(root, "main", "lane"), None);
}
