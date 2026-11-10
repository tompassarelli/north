use super::sampling;

#[test]
fn a_lane_failing_a_mechanical_check_is_bounced_without_a_review() {
    use super::mechanical::{Lane, failing};
    let main = vec!["Add the column".to_owned(), "Revert \"Add a retry\"".to_owned()];
    let oracle = vec!["src/a.rs".to_owned()];
    let lane = |diff: &str, subject: &str, oracle: &[String]| failing(&Lane { diff, subject, main_subjects: &main, oracle_files: oracle }).map(|failure| failure.class);
    let clean = "+++ b/src/a.rs\n+let x = 1;\n";
    assert_eq!(lane(clean, "Add the row", &[]), None);
    assert_eq!(lane(clean, "Add the column", &[]), Some("duplicate"));
    assert_eq!(lane(clean, "Add a retry", &[]), Some("restored-revert"));
    let host_read = format!("+++ b/crates/x/src/tests.rs\n+    let up = std::fs::read(\"/{}/uptime\");\n", "proc");
    assert_eq!(lane(&host_read, "Read it", &[]), Some("test-reads-host"));
    assert_eq!(lane("+++ b/src/a.rs\n+#[test]\n+fn one() {}\n+#[test]\n+fn two() {}\n-#[test]\n", "Test it", &[]), Some("test-budget"));
    assert_eq!(lane("+++ b/src/a.rs\n+#[test]\n-#[test]\n", "Swap a test", &[]), None);
    assert_eq!(lane("+++ b/src/a.rs\n+let x = 2;\n", "Tweak", &oracle), Some("oracle-edit"));
}

#[test]
fn an_oracle_locked_box_is_sampled_from_its_first_run() {
    let (unsampled, sampled) = ("00000001", "00000005");
    assert!(sampling::reviews(&[], "feature", false, false, unsampled), "an unlocked category with no clean review is reviewed in full");
    assert!(!sampling::reviews(&[], "feature", false, true, unsampled), "an oracle-locked box with no clean review sends an unsampled sha to the lander");
    assert!(sampling::reviews(&[], "feature", false, true, sampled), "an oracle-locked box still has one sha in five reviewed");
    assert!(sampling::reviews(&[], "feature", true, true, unsampled), "an oracle-locked landing box is still reviewed in full");
}
