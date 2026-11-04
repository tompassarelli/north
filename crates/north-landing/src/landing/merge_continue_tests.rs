use std::fs;

use super::*;

const CONFIG: &str = "[[set]]\npath = \"index.txt\"\nkind = \"regenerate\"\ncommand = \"sh gen.sh\"\nsources = [\"src/\"]\n";

fn ok(root: &Path, arguments: &[&str]) {
    let output = git(root, arguments).unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn add_item(root: &Path, item: &str) {
    fs::write(root.join("src").join(item), "x\n").unwrap();
    assert!(Command::new("sh").arg("gen.sh").current_dir(root).status().unwrap().success());
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-qm", item]);
}

fn scratch() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false"), ("merge.set.driver", "false")] {
        ok(root, &["config", key, value]);
    }
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("gen.sh"), "ls src > index.txt\n").unwrap();
    fs::write(root.join("sets.toml"), CONFIG).unwrap();
    fs::write(root.join(".gitattributes"), "index.txt merge=set\nlist.txt merge=set\n").unwrap();
    fs::write(root.join("list.txt"), "a\n").unwrap();
    add_item(root, "a");
    ok(root, &["checkout", "-q", "-b", "lane"]);
    add_item(root, "c");
    add_item(root, "d");
    ok(root, &["checkout", "-q", "main"]);
    add_item(root, "b");
    dir
}

fn assert_finished_with_every_item(root: &Path) {
    assert_eq!(in_progress(root).unwrap(), None);
    assert_eq!(fs::read_to_string(root.join("index.txt")).unwrap(), "a\nb\nc\nd\n");
    assert_eq!(stdout(root, &["status", "--porcelain"]).unwrap(), "");
}

#[test]
fn a_rebase_stopped_on_a_regenerated_file_finishes_with_the_merged_sources_at_every_commit() {
    let dir = scratch();
    let root = dir.path();
    ok(root, &["checkout", "-q", "lane"]);
    assert!(!git(root, &["rebase", "main"]).unwrap().status.success());
    finish(root, &root.join("sets.toml")).unwrap();
    assert_finished_with_every_item(root);
    assert_eq!(stdout(root, &["show", "HEAD~1:index.txt"]).unwrap(), "a\nb\nc\n");
    assert_eq!(stdout(root, &["rev-list", "--count", "main..HEAD"]).unwrap().trim(), "2");
}

#[test]
fn a_cherry_picked_range_stopped_on_a_regenerated_file_finishes() {
    let dir = scratch();
    let root = dir.path();
    assert!(!git(root, &["cherry-pick", "main~1..lane"]).unwrap().status.success());
    finish(root, &root.join("sets.toml")).unwrap();
    assert_finished_with_every_item(root);
    assert_eq!(stdout(root, &["rev-list", "--count", "lane~2..HEAD"]).unwrap().trim(), "3");
}

#[test]
fn a_conflict_in_a_file_that_is_not_regenerated_is_refused_by_name_and_left_in_progress() {
    let dir = scratch();
    let root = dir.path();
    fs::write(root.join("list.txt"), "main\n").unwrap();
    ok(root, &["commit", "-qam", "main list"]);
    ok(root, &["checkout", "-q", "lane"]);
    fs::write(root.join("list.txt"), "lane\n").unwrap();
    ok(root, &["commit", "-qam", "lane list"]);
    assert!(!git(root, &["rebase", "main"]).unwrap().status.success());
    let error = finish(root, &root.join("sets.toml")).unwrap_err();
    assert!(error.contains("list.txt"), "{error}");
    assert_eq!(in_progress(root).unwrap(), Some(Operation::Rebase));
}

#[test]
fn finish_outside_a_rebase_or_cherry_pick_does_nothing() {
    let dir = scratch();
    finish(dir.path(), &dir.path().join("sets.toml")).unwrap();
}

fn add_source_only(root: &Path, item: &str) {
    fs::write(root.join("src").join(item), "x\n").unwrap();
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-qm", item]);
}

#[test]
fn refresh_folds_a_regenerated_index_into_a_lane_that_added_a_source_without_it() {
    let dir = scratch();
    let root = dir.path();
    ok(root, &["checkout", "-q", "-b", "stale", "main~1"]);
    add_source_only(root, "e");
    ok(root, &["checkout", "-q", "main"]);
    let onto = stdout(root, &["rev-parse", "HEAD"]).unwrap();
    ok(root, &["cherry-pick", "main~1..stale"]);
    let message = stdout(root, &["log", "-1", "--format=%s"]).unwrap();
    assert_eq!(refresh(root, &root.join("sets.toml"), onto.trim()).unwrap(), ["index.txt"]);
    assert_eq!(fs::read_to_string(root.join("index.txt")).unwrap(), "a\nb\ne\n");
    assert_eq!(stdout(root, &["show", "HEAD:index.txt"]).unwrap(), "a\nb\ne\n");
    assert_eq!(stdout(root, &["log", "-1", "--format=%s"]).unwrap(), message);
    assert_eq!(stdout(root, &["rev-list", "--count", &format!("{}..HEAD", onto.trim())]).unwrap().trim(), "1");
}

#[test]
fn refresh_leaves_a_lane_that_committed_its_own_index_or_touched_no_source() {
    let dir = scratch();
    let root = dir.path();
    let onto = stdout(root, &["rev-parse", "main"]).unwrap();
    ok(root, &["checkout", "-q", "lane"]);
    assert!(!git(root, &["rebase", "main"]).unwrap().status.success());
    finish(root, &root.join("sets.toml")).unwrap();
    let head = stdout(root, &["rev-parse", "HEAD"]).unwrap();
    assert!(refresh(root, &root.join("sets.toml"), onto.trim()).unwrap().is_empty());
    fs::write(root.join("list.txt"), "z\n").unwrap();
    ok(root, &["commit", "-qam", "list only"]);
    let before = stdout(root, &["rev-parse", "HEAD"]).unwrap();
    assert!(refresh(root, &root.join("sets.toml"), head.trim()).unwrap().is_empty());
    assert_eq!(stdout(root, &["rev-parse", "HEAD"]).unwrap(), before);
    assert!(refresh(root, &root.join("missing.toml"), onto.trim()).unwrap().is_empty());
}
