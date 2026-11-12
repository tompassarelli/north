use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use north_base::platform::wait::timed;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).current_dir(dir).output().expect("git");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_classifier_slower_than_the_deadline_leaves_the_launch_without_a_prediction() {
    let top = tempfile::tempdir().expect("tempdir");
    let repo = top.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("src");
    std::fs::write(repo.join("src/lib.rs"), "pub fn named() {}\n").expect("lib");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "src/lib.rs"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    let north = top.path().join("north");
    std::fs::write(&north, "#!/bin/sh\nsleep 4\necho '{\"text\":\"src/lib.rs\"}'\n").expect("north");
    std::fs::set_permissions(&north, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let (answer, waited) = timed(|| section("Make the parser faster", &repo, None, &north));
    assert!(waited < Duration::from_secs(3), "the launch waited {waited:?} for the classifier");
    assert!(answer.is_some_and(|(_, predicted)| predicted.writes.is_empty()), "the launch went on with a prediction the classifier answered late");
}
