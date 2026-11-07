use std::fs;
use std::time::SystemTime;

use super::rescue::{Finished, Rescue, apply_diff};
use super::test_git::{git_in, init_repo};

#[test]
fn a_clean_main_ahead_of_origin_keeps_its_commits_on_a_keep_branch_and_resets() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let main = base.join("main");
    init_repo(&main, None);
    git_in(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git_in(
        &main,
        &[
            "remote",
            "add",
            "origin",
            base.join("origin.git").to_str().unwrap(),
        ],
    );
    git_in(&main, &["push", "-q", "origin", "main"]);
    git_in(&base, &["clone", "-q", "origin.git", "peer"]);
    let peer = base.join("peer");
    fs::write(peer.join("peer.txt"), "peer\n").unwrap();
    git_in(&peer, &["add", "peer.txt"]);
    git_in(&peer, &["commit", "-qm", "peer"]);
    git_in(&peer, &["push", "-q", "origin", "main"]);
    fs::write(main.join("local.txt"), "local\n").unwrap();
    git_in(&main, &["add", "local.txt"]);
    git_in(&main, &["commit", "-qm", "local"]);
    let local = git_in(&main, &["rev-parse", "HEAD"]);
    let rescue = || {
        Rescue {
            now: SystemTime::now(),
            apply_diff,
        }
        .rescue(&main, false)
    };
    let keeps = || {
        git_in(
            &main,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads/keep/",
            ],
        )
    };

    let Finished::Done(line) = rescue() else {
        panic!("a clean main ahead of origin must be rescued")
    };
    let origin = git_in(&main, &["rev-parse", "origin/main"]);
    assert_eq!(git_in(&main, &["rev-parse", "HEAD"]), origin);
    assert_eq!(git_in(&main, &["status", "--porcelain"]), "");
    let keep = keeps();
    assert!(
        keep.starts_with("keep/") && !keep.contains('\n') && line.contains(&keep),
        "{keep}: {line}"
    );
    assert_eq!(git_in(&main, &["rev-parse", &keep]), local);

    git_in(&main, &["reset", "-q", "--hard", &keep]);
    let Finished::Done(_) = rescue() else {
        panic!("a second rescue must succeed")
    };
    assert_eq!(git_in(&main, &["rev-parse", "HEAD"]), origin);
    assert_eq!(keeps(), keep);
}
