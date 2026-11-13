use std::collections::BTreeSet;

use north_build_system::TestRun;

use super::{Verdict, verdict};

fn target(name: &str, printed: &str) -> TestRun {
    TestRun { target: format!("root//crates/{name}:{name}-unittest"), action: None, reused: false, passed: true, seconds: 0.1, printed: format!("---- STDOUT ----\n{printed}\n---- STDERR ----\n\n") }
}

fn ran(cases: u32) -> String {
    format!("     Summary [   0.154s] {cases} tests run: {cases} passed, 0 failed, 0 skipped")
}

#[test]
fn buck2_passes_a_crate_only_on_cases_its_own_targets_record_and_fails_the_check_on_a_failed_build_or_a_counted_failure_north_1299() {
    let summary = |passed: u32, failed: u32| format!("Tests finished: \u{1b}[38;5;10mPass {passed}\u{1b}[39m. Fail {failed}. Timeout 0. Fatal 0. Skip 0. Omit 0. Infra Failure 3. Build failure 0");
    let owner = |label: &str| label.strip_prefix("root//crates/").and_then(|target| target.split_once(':')).map(|(name, _)| name.to_owned()).filter(|name| name != "z");
    let said = |status: i32, summary: Option<String>, runs: Option<&[TestRun]>| verdict(Some(status), summary.as_deref(), runs, &owner);
    let passed = |names: &[&str]| Verdict::Passed(names.iter().map(|name| (*name).to_owned()).collect());
    let unpassed = TestRun { passed: false, ..target("e", &ran(4)) };
    let record = [target("a", &ran(14)), target("b", &ran(0)), target("c", ""), target("d", &ran(2)), target("d", &ran(1)), unpassed, target("z", &ran(3))];
    assert_eq!(said(0, Some(summary(7, 0)), Some(&record)), passed(&["a", "d"]), "a crate whose targets ran 14 or 3 cases passes; one whose target ran none, printed no summary or did not pass stays on the declared line, and a run of a crate that did not move passes nothing (north#1299)");
    assert_eq!(said(0, Some(summary(8, 0)), Some(&record)), passed(&[]), "a passed target missing from the record leaves every crate on the declared line");
    assert_eq!((said(0, Some(summary(7, 0)), None), said(0, None, Some(&record))), (passed(&[]), passed(&[])), "as does an unreadable record or an exit 0 with no summary");
    assert_eq!(said(3, None, None), Verdict::Failed, "a build the tree failed");
    assert_eq!(said(32, Some(summary(4, 1)), None), Verdict::Failed, "a test the summary counts as failed");
    assert_eq!(said(32, Some(summary(0, 0)), None), passed(&[]), "a test run lost to the workers, with no failed test counted, is not a failure of the tree");
    assert_eq!((said(2, Some(summary(5, 0)), Some(&record)), verdict(None, None, None, &owner)), (passed(&[]), Verdict::Passed(BTreeSet::new())), "any other status is the engine's");
}
