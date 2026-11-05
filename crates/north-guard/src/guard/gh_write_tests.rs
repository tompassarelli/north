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

