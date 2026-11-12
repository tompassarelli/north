use super::{Verdict, verdict};

#[test]
fn buck2_decides_a_crate_s_tests_only_on_a_counted_pass_a_failed_build_or_a_counted_failure() {
    let summary = |passed: u32, failed: u32| format!("Tests finished: \u{1b}[38;5;10mPass {passed}\u{1b}[39m. Fail {failed}. Timeout 0. Fatal 0. Skip 0. Omit 0. Infra Failure 3. Build failure 0");
    let said = |status: i32, summary: Option<String>| verdict(Some(status), summary.as_deref());
    assert_eq!(said(0, Some(summary(1, 0))), Verdict::Passed);
    assert_eq!(said(0, Some(summary(0, 0))), Verdict::Silent, "a run that passed no test says nothing of the crate, which would otherwise go untested");
    assert_eq!(said(0, None), Verdict::Silent, "nor does an exit 0 with no summary");
    assert_eq!(said(3, None), Verdict::Failed, "a build the tree failed");
    assert_eq!(said(32, Some(summary(4, 1))), Verdict::Failed, "a test the summary counts as failed");
    assert_eq!(said(32, Some(summary(0, 0))), Verdict::Silent, "a test run lost to the workers, with no failed test counted, is not a failure of the tree");
    assert_eq!((said(2, Some(summary(1, 0))), verdict(None, None)), (Verdict::Silent, Verdict::Silent), "any other status is the engine's");
}
