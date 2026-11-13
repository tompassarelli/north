use north_build_system::TestRun;

use super::{Verdict, verdict};

fn target(printed: &str) -> TestRun {
    TestRun { target: "root//crates/north-buck:north-buck-unittest".to_owned(), action: None, reused: false, passed: true, seconds: 0.1, printed: format!("---- STDOUT ----\n{printed}\n---- STDERR ----\n\n") }
}

#[test]
fn buck2_decides_a_crate_s_tests_only_on_cases_its_record_counts_a_failed_build_or_a_counted_failure_north_1299() {
    let summary = |passed: u32, failed: u32| format!("Tests finished: \u{1b}[38;5;10mPass {passed}\u{1b}[39m. Fail {failed}. Timeout 0. Fatal 0. Skip 0. Omit 0. Infra Failure 3. Build failure 0");
    let fourteen = [target("     Summary [   0.154s] 14 tests run: 14 passed, 0 failed, 0 skipped")];
    let none = [target("     Summary [   0.002s] 0 tests run: 0 passed, 0 failed, 0 skipped")];
    let said = |status: i32, summary: Option<String>, runs: Option<&[TestRun]>| verdict(Some(status), summary.as_deref(), runs);
    assert_eq!(said(0, Some(summary(1, 0)), Some(&fourteen)), Verdict::Passed);
    assert_eq!(said(0, Some(summary(1, 0)), Some(&none)), Verdict::Silent, "a target Buck2 counts as passed whose runner ran no case says nothing of the crate, which would otherwise go untested");
    assert_eq!(said(0, Some(summary(1, 0)), Some(&[target("")])), Verdict::Silent, "nor does a target whose runner printed no summary");
    assert_eq!(said(0, Some(summary(2, 0)), Some(&fourteen)), Verdict::Silent, "nor a passed target missing from the record");
    assert_eq!((said(0, Some(summary(1, 0)), None), said(0, None, Some(&fourteen))), (Verdict::Silent, Verdict::Silent), "nor an unreadable record or an exit 0 with no summary");
    assert_eq!(said(3, None, None), Verdict::Failed, "a build the tree failed");
    assert_eq!(said(32, Some(summary(4, 1)), None), Verdict::Failed, "a test the summary counts as failed");
    assert_eq!(said(32, Some(summary(0, 0)), None), Verdict::Silent, "a test run lost to the workers, with no failed test counted, is not a failure of the tree");
    assert_eq!((said(2, Some(summary(1, 0)), Some(&fourteen)), verdict(None, None, None)), (Verdict::Silent, Verdict::Silent), "any other status is the engine's");
}
