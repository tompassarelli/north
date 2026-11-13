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
fn a_squashed_two_commit_lane_lands_under_its_stated_or_first_subject() {
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
    commit(root, "a.txt", "Read both yq error forms in the frontmatter check\n\nyq-error-forms body line\n\nItem: north#1\nMain-red: yq at abc");
    commit(root, "b.txt", "Fix the typo the review found\n\nreview-typo body line\n\nMain-red: yq at abc");
    let lane = git_ok(root, &["rev-parse", "HEAD"]);

    assert!(matches!(squash(root, "main", None).unwrap(), Squash::Squashed { commits: 2, .. }));
    let landed = git_ok(root, &["show", "-s", "--format=%B", "HEAD"]);
    let expected = "Read both yq error forms in the frontmatter check\n\nyq-error-forms body line\n\n\
        Fix the typo the review found\n\nreview-typo body line\n\n\
        Item: north#1\nMain-red: yq at abc";
    assert_eq!(landed, expected, "697193f179 landed under its late fixup's subject");

    git_ok(root, &["reset", "-q", "--hard", &lane]);
    assert!(matches!(squash(root, "main", Some("Read yq's two error forms\n\nStated body.\n\nTest-contract: sync\n")).unwrap(), Squash::Squashed { commits: 2, .. }));
    let landed = git_ok(root, &["show", "-s", "--format=%B", "HEAD"]);
    assert_eq!(landed, "Read yq's two error forms\n\nStated body.\n\nTest-contract: sync\nItem: north#1\nMain-red: yq at abc");
}
