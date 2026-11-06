use super::*;

const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3 +3 @@ fn a() {\n-old\n+new\n@@ -10,0 +11,2 @@ fn b() {\n+one\n+two\ndiff --git a/src/gone.rs b/src/gone.rs\n--- a/src/gone.rs\n+++ /dev/null\n@@ -1,4 +0,0 @@\n-x\n";

#[test]
fn a_span_shifts_past_earlier_hunks_and_goes_stale_when_a_hunk_lands_inside_it() {
    let hunks = parse(DIFF).remove("src/a.rs").unwrap();
    assert_eq!(carry(1, 2, &hunks), Some((1, 2)));
    assert_eq!(carry(2, 4, &hunks), None);
    assert_eq!(carry(5, 10, &hunks), Some((5, 10)));
    assert_eq!(carry(9, 12, &hunks), None);
    assert_eq!(carry(11, 20, &hunks), Some((13, 22)));
}

#[test]
fn a_removed_line_that_starts_with_two_dashes_is_content_not_a_file_header() {
    let files = parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3 +2,0 @@\n--- comment\n@@ -9 +8 @@\n-x\n+y\n");
    assert_eq!(files["src/a.rs"].len(), 2);
}
