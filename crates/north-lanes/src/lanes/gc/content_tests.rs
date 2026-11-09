use std::fs;
use std::path::Path;

use super::content::MainContent;
use super::evidence::landed;
use crate::lanes::test_git::{git_in, init_repo};

fn commit(repo: &Path, file: &str, text: &str, subject: &str) {
    fs::write(repo.join(file), text).unwrap();
    git_in(repo, &["add", file]);
    git_in(repo, &["commit", "-qm", subject]);
}

#[test]
fn a_lane_on_rewritten_history_with_its_patches_on_main_is_landed() {
    let root = tempfile::tempdir().unwrap();
    let main = root.path().canonicalize().unwrap().join("main");
    init_repo(&main, None);
    commit(&main, "h.txt", "history main kept\n", "h");
    commit(&main, "g.txt", "the lane's work\n", "g, as the lander landed it");
    git_in(&main, &["checkout", "-q", "--orphan", "rewritten"]);
    git_in(&main, &["rm", "-rq", "--cached", "."]);
    fs::remove_file(main.join("h.txt")).unwrap();
    fs::remove_file(main.join("g.txt")).unwrap();
    commit(&main, "f.txt", "base\n", "base, before history was rewritten");
    commit(&main, "g.txt", "the lane's work\n", "g, on the old history");
    let tip = git_in(&main, &["rev-parse", "HEAD"]);
    git_in(&main, &["checkout", "-q", "main"]);

    assert_eq!(MainContent::read(&main, "main").holds(&main, "main", &tip), Some("patches on main"));
    assert!(landed(&main, "main", &tip).landed);

    git_in(&main, &["checkout", "-q", "rewritten"]);
    commit(&main, "k.txt", "never landed\n", "k");
    let unlanded = git_in(&main, &["rev-parse", "HEAD"]);
    git_in(&main, &["checkout", "-q", "main"]);
    assert_eq!(MainContent::read(&main, "main").holds(&main, "main", &unlanded), None, "a patch main lacks keeps the lane unlanded");
}

#[test]
fn a_second_gc_run_diffs_only_main_commits_added_since_the_first() {
    let root = tempfile::tempdir().unwrap();
    let main = root.path().canonicalize().unwrap().join("main");
    init_repo(&main, None);
    commit(&main, "h.txt", "history main kept\n", "h");
    let base = git_in(&main, &["rev-parse", "HEAD"]);
    git_in(&main, &["checkout", "-q", "-b", "lane", &base]);
    commit(&main, "g.txt", "the lane's work\n", "g, on the lane");
    let tip = git_in(&main, &["rev-parse", "HEAD"]);
    git_in(&main, &["checkout", "-q", "main"]);
    commit(&main, "m.txt", "main moved on\n", "m, before the lane landed");
    commit(&main, "g.txt", "the lane's work\n", "g, as the lander landed it");

    let first = MainContent::read(&main, "main");
    assert_eq!(first.diffed(), 4, "a run with no cache diffs every commit on main");
    assert_eq!(first.holds(&main, "main", &tip), Some("patches on main"));
    assert_eq!(MainContent::read(&main, "main").diffed(), 0, "a run with nothing new on main diffs no commit");

    commit(&main, "k.txt", "main moves on\n", "k");
    let second = MainContent::read(&main, "main");
    assert_eq!(second.diffed(), 1, "only the commit main gained since the first run is diffed");
    assert_eq!(second.holds(&main, "main", &tip), Some("patches on main"), "the cached patch ids still retire a lane whose patches are on main");

    git_in(&main, &["reset", "-q", "--hard", &base]);
    assert_eq!(MainContent::read(&main, "main").holds(&main, "main", &tip), None, "a cached patch id of a commit main no longer holds retires nothing");
}
