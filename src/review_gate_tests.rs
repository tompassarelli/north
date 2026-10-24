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
    assert_eq!(verdict(root, &base, "HEAD", &paths), Ok(Verdict::Uncovered));

    std::fs::write(root.join("harness/rule.md"), b"top\na\nb\nc\none\ntwo, reviewed \xe9\nthree\n").unwrap();
    commit(root, "Change the rule");
    let reviewed = patch_id(root, &base, "HEAD", &paths).unwrap().unwrap();
    let Ok(Verdict::Missing { landing }) = verdict(root, &base, "HEAD", &paths) else {
        panic!("an unreviewed covered change passed");
    };
    assert_eq!(landing, reviewed);
    assert!(refusal(&Verdict::Missing { landing }, &base, "HEAD", &paths).unwrap().contains(&reviewed));

    git(root, &["-c", "commit.gpgsign=false", "commit", "--amend", "-qm",
        &format!("Change the rule\n\nReviewed: fresh reviewer, ACCEPT; patch-id {reviewed}")]).unwrap();
    assert_eq!(verdict(root, &base, "HEAD", &paths), Ok(Verdict::Reviewed));
    write(root, "docs/notes.md", "notes after review\n");
    commit(root, "Change notes after the review");
    assert_eq!(verdict(root, &base, "HEAD", &paths), Ok(Verdict::Reviewed));

    git(root, &["switch", "-q", "main"]).unwrap();
    write(root, "harness/rule.md", "top\na\nb\nc, moved on main\none\ntwo\nthree\n");
    let moved = commit(root, "Move main");
    git(root, &["switch", "-q", "lane"]).unwrap();
    git(root, &["-c", "commit.gpgsign=false", "rebase", "-q", "main"]).unwrap();
    assert_eq!(verdict(root, &moved, "HEAD", &paths), Ok(Verdict::Reviewed));

    std::fs::write(root.join("harness/rule.md"), b"top\na\nb\nc, moved on main\none\ntwo, reviewed \xe9\nthree, after the review\n").unwrap();
    commit(root, "Change the rule after the review");
    let stale = verdict(root, &moved, "HEAD", &paths).unwrap();
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
