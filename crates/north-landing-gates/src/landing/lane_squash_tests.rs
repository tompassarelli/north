use super::*;

fn git_ok(root: &Path, args: &[&str]) -> String {
    git(root, args).unwrap_or_else(|error| panic!("{error}")).trim().to_owned()
}

fn commit(root: &Path, file: &str, message: &str) {
    std::fs::write(root.join(file), message).unwrap();
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["-c", "commit.gpgsign=false", "commit", "-qm", message]);
}

#[test]
fn a_squash_without_message_keeps_every_commit_explanation_and_trailers_once() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.name", "fixture"]);
    git_ok(root, &["config", "user.email", "fixture@example.invalid"]);
    commit(root, "base.txt", "base");
    let remote = tempfile::tempdir().unwrap();
    git_ok(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    git_ok(root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    git_ok(root, &["push", "-q", "origin", "main"]);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit(root, "a.txt", "Return a clean checkout parked on a landed branch to main\n\nsync-heal-branch body line\n\nTest-contract: sync");
    commit(root, "b.txt", "Read both yq error forms in the frontmatter check\n\nyq-error-forms body line\n\nMain-red: yq at abc");
    commit(root, "c.txt", "Check a red-fix ticket before every urgent one\n\nred-fix-first body line\n\nMain-red: yq at abc");
    commit(root, "d.txt", "Land an empty tip\n\nlanded-empty-tip body line\n\nMain-red: tip at def");
    let outcome = squash(root, "main", None).unwrap();
    assert!(matches!(outcome, Squash::Squashed { commits: 4, .. }));
    let landed = git_ok(root, &["show", "-s", "--format=%B", "HEAD"]);
    let expected = "Land an empty tip\n\nlanded-empty-tip body line\n\n\
        Return a clean checkout parked on a landed branch to main\n\nsync-heal-branch body line\n\n\
        Read both yq error forms in the frontmatter check\n\nyq-error-forms body line\n\n\
        Check a red-fix ticket before every urgent one\n\nred-fix-first body line\n\n\
        Test-contract: sync\nMain-red: yq at abc\nMain-red: tip at def";
    assert_eq!(landed, expected);
}
