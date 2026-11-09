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
