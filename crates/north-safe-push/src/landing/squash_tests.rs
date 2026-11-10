use std::path::Path;
use std::process::Command;

use north_lander::landing::lane_squash::{Squash, squash};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, file: &str, text: &str, message: &str) {
    std::fs::write(root.join(file), text).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
}

#[test]
fn a_lane_pushed_again_after_its_first_commit_landed_keeps_the_later_deletion() {
    let scratch = tempfile::tempdir().unwrap();
    let (origin, root) = (scratch.path().join("origin.git"), scratch.path().join("lane"));
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(scratch.path(), &["init", "-q", "-b", "main", root.to_str().unwrap()]);
    git(&root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    commit(&root, "router.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n", "base");
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["checkout", "-q", "-b", "lane"]);
    commit(&root, "router.rs", "one\ntwo\nthree\nheld\nfour\nfive\nsix\nseven\n", "add the held arm");
    let first = git(&root, &["rev-parse", "HEAD"]);
    commit(&root, "router.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n", "move the held verb and delete its arm");
    git(&root, &["checkout", "-q", "main"]);
    commit(&root, "other.rs", "elsewhere\n", "main moves on");
    git(&root, &["cherry-pick", &first]);
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["checkout", "-q", "lane"]);
    let main = git(&root, &["rev-parse", "origin/main"]);
    assert!(matches!(squash(&root, &main, None), Ok(Squash::Unchanged | Squash::Squashed { .. })));
    git(&root, &["checkout", "-q", "--detach", &main]);
    git(&root, &["cherry-pick", "--allow-empty", "--empty=drop", &format!("{main}..lane")]);
    assert_eq!(std::fs::read_to_string(root.join("router.rs")).unwrap(), "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
}
