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
fn rebased_then_landed_lane_is_landed_by_tree() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta changed\ngamma\n");
    commit(root, "other.txt", "unrelated\n", "main moves");
    commit(root, "app.txt", "alpha\nbeta changed\ngamma\n", "land rebased copy");
    assert_eq!(landed(root, "main", "lane"), Some(Verdict::TreeOnMain));
}

#[test]
fn landed_lane_later_rewritten_on_main_is_superseded() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta changed\ngamma\n");
    commit(root, "other.txt", "unrelated\n", "main moves");
    commit(root, "app.txt", "alpha\nbeta changed\ngamma\n", "lane change");
    commit(root, "app.txt", "alpha\nbeta changed twice\ngamma\n", "later rewrite of the same line");
    assert_eq!(landed(root, "main", "lane"), Some(Verdict::SupersededOnMain));
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

#[test]
fn lane_adding_a_line_main_already_has_elsewhere_in_the_file_is_unlanded() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta\nbeta\ngamma\n");
    commit(root, "app.txt", "alpha\nbeta\ngamma\nzeta\n", "lane change");
    assert_eq!(landed(root, "main", "lane"), None);
}

#[test]
fn lines_superseded_on_main_without_the_lane_commit_message_are_unlanded() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta changed\ngamma\n");
    commit(root, "app.txt", "alpha\nbeta changed\ngamma\n", "an unrelated message");
    commit(root, "app.txt", "alpha\nbeta changed twice\ngamma\n", "later rewrite of the same line");
    assert_eq!(landed(root, "main", "lane"), None);
}

#[test]
fn a_main_commit_with_the_lane_subject_but_other_paths_does_not_carry_the_lane() {
    let scratch = fixture();
    let root = scratch.path();
    lane_edit(root, "alpha\nbeta changed\ngamma\n");
    commit(root, "other.txt", "unrelated\n", "lane change");
    commit(root, "app.txt", "alpha\nbeta changed\ngamma\n", "an unrelated message");
    commit(root, "app.txt", "alpha\nbeta changed twice\ngamma\n", "later rewrite of the same line");
    assert_eq!(landed(root, "main", "lane"), None);
}
