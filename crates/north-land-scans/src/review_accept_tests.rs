use std::path::Path;

use super::*;
use north_base::platform::finish_record::Finished;
use north_base::platform::review_accept::Accept;

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_withdrawn_accept_no_longer_counts() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[("crates/north-lander/src/land.rs", "a\n")], "base");
    let lane = commit(&repo, &[("crates/north-lander/src/land.rs", "b\n")], "inside the boundary");
    let test = commit(&repo, &[("crates/north-lander/tests/land.rs", "t\n")], "a test");
    accept(&accepts, &repo, &base, &lane, "reviewer-a");
    accept(&accepts, &repo, &base, &lane, "reviewer-b");
    assert_eq!(check(&repo, &base, &lane, &accepts, &[]), Ok(()));
    let patch = patch_id(&repo, &base, &lane).unwrap();
    assert_eq!(accepts.withdraw(&patch, "reviewer-b"), Ok(true));
    assert!(check(&repo, &base, &lane, &accepts, &[]).unwrap_err().contains("holds 1 of 2 review accepts"), "a withdrawn accept still counted");
    assert_eq!(accepts.withdraw(&patch, "reviewer-b"), Ok(false), "a second withdrawal finds nothing");

    accept(&accepts, &repo, &lane, &test, "reviewer-b");
    assert_eq!(check(&repo, &lane, &test, &accepts, &[]), Ok(()));
    assert_eq!(accepts.withdraw(&patch_id(&repo, &lane, &test).unwrap(), "reviewer-b"), Ok(true));
    assert!(check(&repo, &lane, &test, &accepts, &[]).is_err(), "a withdrawn accept still landed a one-accept change");
}

fn commit(repo: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", message]);
    git_in(repo, &["rev-parse", "HEAD"])
}

fn accept(accepts: &Accepts, repo: &Path, base: &str, head: &str, session: &str) {
    let fork = git::merge_base(repo, base, head);
    let patch_id = patch_id(repo, &fork, head).unwrap();
    accepts.record(&Accept { patch_id, session: session.to_owned(), at: 1 }).unwrap();
}

const LANDER: &str = "crates/north-lander/src/land.rs";

#[test]
fn a_live_fix_lane_that_changes_verify_north_login_with_no_review_accepts_is_refused_f9f0d1ddf9() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let files = ["README.md", "docs/rationale/0027-product-verification.md", "docs/onboarding.md", "flake.nix", "src/bin/verify_north/login.rs", "src/bin/verify_north/main.rs"];
    let base = commit(&repo, &files.map(|file| (file, "a\n")), "base");
    let lane = commit(&repo, &files.map(|file| (file, "b\n")), "Make the Quickstart one command\n\nLive-fix: launch gate for north#1281");
    let refusal = check(&repo, &base, &lane, &accepts, &[]).unwrap_err();
    assert!(refusal.contains("holds 0 of 2 review accepts") && refusal.contains("src/bin/verify_north/login.rs"), "{refusal}");
    accept(&accepts, &repo, &base, &lane, "reviewer-a");
    assert!(check(&repo, &base, &lane, &accepts, &[]).unwrap_err().contains("holds 1 of 2 review accepts"), "Live-fix does not cut a credential change to one accept");
}

#[test]
fn a_live_fix_speed_cut_lands_on_one_accept() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(LANDER, "a\n")], "base");
    let plain = commit(&repo, &[(LANDER, "b\n")], "a lander change");
    accept(&accepts, &repo, &base, &plain, "reviewer-a");
    assert!(check(&repo, &base, &plain, &accepts, &[]).unwrap_err().contains("holds 1 of 2 review accepts"), "without Live-fix one accept does not land it");
    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let fix = commit(&repo, &[(LANDER, "b\n")], "a lander change\n\nLive-fix: landing p90");
    assert_eq!(check(&repo, &base, &fix, &accepts, &[]), Ok(()), "a Live-fix lane outside two_accept_paths lands on one accept");
    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let claims = commit(&repo, &[("crates/north-fleet/src/fleet/claims.rs", "b\n")], "a claims change\n\nLive-fix: landing p90");
    accept(&accepts, &repo, &base, &claims, "reviewer-a");
    assert!(check(&repo, &base, &claims, &accepts, &[]).unwrap_err().contains("holds 1 of 2 review accepts"), "Live-fix does not cut a claims change to one accept");
}

#[test]
fn a_lane_that_changes_a_trust_boundary_path_lands_with_two_review_accepts_from_distinct_sessions_and_one_for_a_test_or_an_exact_revert_north_1036() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(LANDER, "a\n"), ("src/a.rs", "a\n")], "base");

    let outside = commit(&repo, &[("src/a.rs", "b\n")], "outside the boundary");
    assert_eq!(check(&repo, &base, &outside, &accepts, &[]), Ok(()), "a lane outside trust_boundary_paths needs no accept");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let inside = commit(&repo, &[(LANDER, "b\n")], "inside the boundary");
    assert!(check(&repo, &base, &inside, &accepts, &[]).unwrap_err().contains("holds 0 of 2 review accepts"));
    accept(&accepts, &repo, &base, &inside, "reviewer-a");
    assert!(check(&repo, &base, &inside, &accepts, &[]).unwrap_err().contains("holds 1 of 2 review accepts"), "one accept does not land it");
    accept(&accepts, &repo, &base, &inside, "reviewer-a");
    assert!(check(&repo, &base, &inside, &accepts, &[]).is_err(), "the same session twice is one accept");
    accept(&accepts, &repo, &base, &inside, "reviewer-b");
    assert_eq!(check(&repo, &base, &inside, &accepts, &[]), Ok(()), "two distinct sessions land it");

    git_in(&repo, &["commit", "-q", "--amend", "-m", "the same patch, reworded"]);
    let reworded = git_in(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(check(&repo, &base, &reworded, &accepts, &[]), Ok(()), "the accepts follow the patch, not the commit");

    let author = Finished { session: "author-x".to_owned(), command: String::new(), head: inside.clone(), passed: true, at: 1 };
    let few = Accepts(root.path().join("few"));
    accept(&few, &repo, &base, &inside, "author-x");
    accept(&few, &repo, &base, &inside, "reviewer-a");
    assert_eq!(check(&repo, &base, &inside, &few, &[]), Ok(()), "without a record of who wrote the lane, two sessions count");
    assert!(check(&repo, &base, &inside, &few, std::slice::from_ref(&author)).is_err(), "a session that handed in a commit of the lane is not its reviewer");
    assert!(check(&repo, &base, &reworded, &few, std::slice::from_ref(&author)).is_err(), "nor once the lander has rewritten the commit it handed in");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let test = commit(&repo, &[("crates/north-lander/tests/land.rs", "t\n")], "a test");
    assert!(check(&repo, &base, &test, &accepts, &[]).is_err());
    accept(&accepts, &repo, &base, &test, "reviewer-a");
    assert_eq!(check(&repo, &base, &test, &accepts, &[]), Ok(()), "one accept lands a test-only change");

    git_in(&repo, &["reset", "-q", "--hard", &inside]);
    let revert = commit(&repo, &[(LANDER, "a\n")], &format!("Revert\n\nThis reverts commit {inside}."));
    assert!(check(&repo, &inside, &revert, &accepts, &[]).is_err());
    accept(&accepts, &repo, &inside, &revert, "reviewer-a");
    assert_eq!(check(&repo, &inside, &revert, &accepts, &[]), Ok(()), "one accept lands an exact revert");

    git_in(&repo, &["reset", "-q", "--hard", &inside]);
    let pretend = commit(&repo, &[(LANDER, "c\n")], &format!("Revert\n\nThis reverts commit {inside}."));
    accept(&accepts, &repo, &inside, &pretend, "reviewer-a");
    assert!(check(&repo, &inside, &pretend, &accepts, &[]).is_err(), "a message naming a revert does not make a different patch one");
}

#[test]
fn accepts_recorded_on_the_reviewers_host_count_on_a_build_host_lander() {
    use north_base::platform::records_mirror::{self, ROOT, STAMP, Records, Stamp};
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(LANDER, "a\n")], "base");
    let inside = commit(&repo, &[(LANDER, "b\n")], "inside the boundary");

    let reviewers_state = root.path().join("reviewers-host");
    let recorded = Accepts::of(&[("XDG_STATE_HOME".to_owned(), reviewers_state.display().to_string())], root.path());
    accept(&recorded, &repo, &base, &inside, "reviewer-a");
    accept(&recorded, &repo, &base, &inside, "reviewer-b");

    let build_state = root.path().join("build-host");
    let mirror = build_state.join(ROOT);
    for path in records_mirror::selected(&reviewers_state, std::time::UNIX_EPOCH + records_mirror::KEPT) {
        std::fs::create_dir_all(mirror.join(&path).parent().unwrap()).unwrap();
        std::fs::copy(reviewers_state.join(&path), mirror.join(&path)).unwrap();
    }
    std::fs::write(mirror.join(STAMP), serde_json::to_vec(&Stamp { host: "reviewers-host".to_owned(), state: reviewers_state, at: 1.0 }).unwrap()).unwrap();

    let counted = Accepts::counted(&Records::of(&build_state, "build-host"));
    assert_eq!(check(&repo, &base, &inside, &counted, &[]), Ok(()), "the two accepts recorded on the reviewers' host land the lane on the build host");
}
