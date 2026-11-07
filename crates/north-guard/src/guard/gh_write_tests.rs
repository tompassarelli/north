use super::*;

fn only(command: &str, cwd: Option<&str>) -> GhWrite {
    let mut writes = writes(command, cwd);
    assert_eq!(writes.len(), 1, "{command}: {writes:?}");
    writes.remove(0)
}

#[test]
fn marks_text_it_cannot_see_opaque() {
    assert!(only("printf x | gh issue comment 5 --body-file -", None).opaque);
    assert!(only("gh issue comment 5 --body \"$summary\"", None).opaque);
    assert!(!only("gh issue comment 5 --body 'costs $5'", None).opaque);
    assert!(!only("gh issue comment 5 --body plain", None).opaque);
}


#[test]
fn reads_a_close_reason_and_the_files_it_reads() {
    assert_eq!(only("gh issue close 7 -R a/b -r \"not planned\"", None).reason.as_deref(), Some("not planned"));
    assert_eq!(only("gh issue close 7 --reason=\"not planned\"", None).closes_issue.as_deref(), Some("7"));
    assert!(only("gh issue create --body-file /tmp/n706-missing/body.md", None).unread);
}
