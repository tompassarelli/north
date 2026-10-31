use super::*;

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "."]).unwrap();
    git(root, &["-c", "commit.gpgsign=false", "commit", "-qm", message]).unwrap();
    git(root, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn write(root: &Path, path: &str, text: &str) {
    std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    std::fs::write(root.join(path), text).unwrap();
}

#[test]
fn a_landing_passes_only_with_the_patch_id_of_its_final_covered_diff() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let paths = ["harness/".to_owned()];
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    write(root, "harness/rule.md", "top\na\nb\nc\none\ntwo\nthree\n");
    write(root, "docs/notes.md", "notes\n");
    let base = commit(root, "base");
    git(root, &["switch", "-q", "-c", "lane"]).unwrap();

    write(root, "docs/notes.md", "notes only\n");
    commit(root, "Change notes only");
    assert_eq!(verdict(root, &base, "HEAD", &paths, None), Ok(Verdict::Uncovered));

    std::fs::write(root.join("harness/rule.md"), b"top\na\nb\nc\none\ntwo, reviewed \xe9\nthree\n").unwrap();
    commit(root, "Change the rule");
    let reviewed = patch_id(root, &base, "HEAD", &paths).unwrap().unwrap();
    let Ok(Verdict::Missing { landing }) = verdict(root, &base, "HEAD", &paths, None) else {
        panic!("an unreviewed covered change passed");
    };
    assert_eq!(landing, reviewed);
    assert!(refusal(&Verdict::Missing { landing }, &base, "HEAD", &paths).unwrap().contains(&reviewed));

    git(root, &["-c", "commit.gpgsign=false", "commit", "--amend", "-qm",
        &format!("Change the rule\n\nReviewed: ACCEPT; reviewer: fresh-reviewer; patch-id {reviewed}")]).unwrap();
    assert_eq!(verdict(root, &base, "HEAD", &paths, None), Ok(Verdict::Reviewed));
    write(root, "docs/notes.md", "notes after review\n");
    commit(root, "Change notes after the review");
    assert_eq!(verdict(root, &base, "HEAD", &paths, None), Ok(Verdict::Reviewed));

    git(root, &["switch", "-q", "main"]).unwrap();
    write(root, "harness/rule.md", "top\na\nb\nc, moved on main\none\ntwo\nthree\n");
    let moved = commit(root, "Move main");
    git(root, &["switch", "-q", "lane"]).unwrap();
    git(root, &["-c", "commit.gpgsign=false", "rebase", "-q", "main"]).unwrap();
    assert_eq!(verdict(root, &moved, "HEAD", &paths, None), Ok(Verdict::Reviewed));

    std::fs::write(root.join("harness/rule.md"), b"top\na\nb\nc, moved on main\none\ntwo, reviewed \xe9\nthree, after the review\n").unwrap();
    commit(root, "Change the rule after the review");
    let stale = verdict(root, &moved, "HEAD", &paths, None).unwrap();
    let Verdict::Stale { landing, reviewed: ids } = &stale else {
        panic!("a covered change after the review passed: {stale:?}");
    };
    assert_ne!(landing, &reviewed);
    assert_eq!(ids, std::slice::from_ref(&reviewed));
    let message = refusal(&stale, &moved, "HEAD", &paths).unwrap();
    assert!(message.contains(landing.as_str()) && message.contains(&reviewed), "{message}");

    let mut ids = vec![landing.clone()];
    for text in [
        b"top\na\nb\nc, moved on main\none\ntwo,reviewed \xe9\nthree, after the review\n".as_slice(),
        b"top\na\nb\nc, moved on main\none\ntwo, reviewed \xe9\nthree, after the review\nstop\n",
        b"top\nstop\na\nb\nc, moved on main\none\ntwo, reviewed \xe9\nthree, after the review\n",
    ] {
        std::fs::write(root.join("harness/rule.md"), text).unwrap();
        commit(root, "Edit the rule again");
        let id = patch_id(root, &moved, "HEAD", &paths).unwrap().unwrap();
        assert!(!ids.contains(&id), "a whitespace edit or moved line kept a reviewed id");
        ids.push(id);
    }
}

#[test]
fn a_review_by_the_author_session_or_by_nobody_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let paths = ["harness/".to_owned()];
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    write(root, "harness/rule.md", "one\n");
    let base = commit(root, "base");
    write(root, "harness/rule.md", "two\n");
    commit(root, "Change the rule");
    let id = patch_id(root, &base, "HEAD", &paths).unwrap().unwrap();
    let amend = |trailer: &str| {
        git(root, &["-c", "commit.gpgsign=false", "commit", "--amend", "-qm", &format!("Change the rule\n\n{trailer}")]).unwrap();
    };

    amend(&format!("Reviewed: approve; reviewer: author-lane; patch-id {id}"));
    let denied = verdict(root, &base, "HEAD", &paths, Some("author-lane")).unwrap();
    assert_eq!(denied, Verdict::SelfReviewed { reviewer: "author-lane".into() });
    assert!(refusal(&denied, &base, "HEAD", &paths).unwrap().contains("author-lane"));
    assert_eq!(verdict(root, &base, "HEAD", &paths, Some("other-lane")), Ok(Verdict::Reviewed));
    assert_eq!(verdict(root, &base, "HEAD", &paths, None), Ok(Verdict::Reviewed));

    amend(&format!("Reviewed: reviewer session fresh-one approves; patch-id {id}"));
    assert_eq!(verdict(root, &base, "HEAD", &paths, Some("author-lane")), Ok(Verdict::Reviewed));
    assert_eq!(reviewers(root, &base, "HEAD").unwrap(), ["fresh-one"]);

    amend(&format!("Reviewed: Opus review, approve; patch-id {id}"));
    let anonymous = verdict(root, &base, "HEAD", &paths, Some("author-lane")).unwrap();
    assert_eq!(anonymous, Verdict::Unattributed { landing: id.clone() });
    assert!(refusal(&anonymous, &base, "HEAD", &paths).unwrap().contains("names no reviewer"));
    assert!(reviewers(root, &base, "HEAD").unwrap().is_empty());

    amend(&format!("Reviewed: approve; reviewer: author-lane; patch-id {id}\nReviewed: approve; reviewer: fresh-two; patch-id {id}"));
    assert_eq!(verdict(root, &base, "HEAD", &paths, Some("author-lane")), Ok(Verdict::Reviewed));
    assert_eq!(reviewers(root, &base, "HEAD").unwrap(), ["author-lane", "fresh-two"]);
}

#[test]
fn only_a_named_reviewer_counts_and_the_author_matches_in_any_case() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let paths = ["harness/".to_owned()];
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    write(root, "harness/rule.md", "one\n");
    let base = commit(root, "base");
    write(root, "harness/rule.md", "two\n");
    commit(root, "Change the rule");
    let id = patch_id(root, &base, "HEAD", &paths).unwrap().unwrap();
    for (trailer, reviewer) in [
        ("Reviewed: fresh reviewer approves", None),
        ("Reviewed: a reviewer of a different tier approves", None),
        ("Reviewed: Tom (dictated 2026-10-27)", Some("Tom")),
        ("Reviewed: approve; Reviewer: Fresh-One", Some("Fresh-One")),
    ] {
        git(root, &["-c", "commit.gpgsign=false", "commit", "--amend", "-qm", &format!("Change the rule\n\n{trailer}; patch-id {id}")]).unwrap();
        assert_eq!(reviews(&range_messages(root, &base, "HEAD").unwrap())[0].reviewer.as_deref(), reviewer, "{trailer}");
    }
    let amended = format!("Change the rule\n\nReviewed: approve; reviewer: Author-Lane; patch-id {id}");
    git(root, &["-c", "commit.gpgsign=false", "commit", "--amend", "-qm", &amended]).unwrap();
    assert_eq!(
        verdict(root, &base, "HEAD", &paths, Some("author-lane")),
        Ok(Verdict::SelfReviewed { reviewer: "Author-Lane".into() })
    );
}
