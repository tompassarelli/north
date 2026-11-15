use std::path::Path;

use north_base::test_inputs::git::git_in;
use north_lander::landing::lane_squash::{Squash, squash};

fn commit(root: &Path, file: &str, text: &str, message: &str) {
    std::fs::write(root.join(file), text).unwrap();
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-qm", message]);
}

#[test]
fn a_lane_pushed_again_after_its_first_commit_landed_keeps_the_later_deletion() {
    let scratch = tempfile::tempdir().unwrap();
    let (origin, root) = (scratch.path().join("origin.git"), scratch.path().join("lane"));
    git_in(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git_in(scratch.path(), &["init", "-q", "-b", "main", root.to_str().unwrap()]);
    git_in(&root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    commit(&root, "router.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n", "base");
    git_in(&root, &["push", "-q", "origin", "main"]);
    git_in(&root, &["checkout", "-q", "-b", "lane"]);
    commit(&root, "router.rs", "one\ntwo\nthree\nheld\nfour\nfive\nsix\nseven\n", "add the held arm");
    let first = git_in(&root, &["rev-parse", "HEAD"]);
    commit(&root, "router.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n", "move the held verb and delete its arm");
    git_in(&root, &["checkout", "-q", "main"]);
    commit(&root, "other.rs", "elsewhere\n", "main moves on");
    git_in(&root, &["cherry-pick", &first]);
    git_in(&root, &["push", "-q", "origin", "main"]);
    git_in(&root, &["checkout", "-q", "lane"]);
    let main = git_in(&root, &["rev-parse", "origin/main"]);
    assert!(matches!(squash(&root, &main, None), Ok(Squash::Unchanged | Squash::Squashed { .. })));
    git_in(&root, &["checkout", "-q", "--detach", &main]);
    git_in(&root, &["cherry-pick", "--allow-empty", "--empty=drop", &format!("{main}..lane")]);
    assert_eq!(std::fs::read_to_string(root.join("router.rs")).unwrap(), "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
}
