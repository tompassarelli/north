use std::collections::BTreeSet;

use north_build_system::TestRun;

use super::{Verdict, infrastructure_record, remote_error, verdict};

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
    let said = |status: i32, summary: Option<String>, runs: Option<&[TestRun]>| verdict(Some(status), summary.as_deref(), runs, &owner, &BTreeSet::new(), false);
    let passed = |names: &[&str]| Verdict::Passed(names.iter().map(|name| (*name).to_owned()).collect());
    let unpassed = TestRun { passed: false, ..target("e", &ran(4)) };
    let record = [target("a", &ran(14)), target("b", &ran(0)), target("c", ""), target("d", &ran(2)), target("d", &ran(1)), unpassed, target("z", &ran(3))];
    assert_eq!(said(0, Some(summary(7, 0)), Some(&record)), passed(&["a", "d"]), "a crate whose targets ran 14 or 3 cases passes; one whose target ran none, printed no summary or did not pass stays on the declared line, and a run of a crate that did not move passes nothing (north#1299)");
    let caseless: BTreeSet<String> = ["b", "c", "d", "e"].map(str::to_owned).into();
    assert_eq!(verdict(Some(0), Some(&summary(7, 0)), Some(&record), &owner, &caseless, false), passed(&["a", "b", "d"]), "a crate the host names caseless passes when its targets passed with no case run, not when one printed no summary or did not pass, and one whose cases ran is judged as any other");
    assert_eq!(said(0, Some(summary(8, 0)), Some(&record)), passed(&[]), "a passed target missing from the record leaves every crate on the declared line");
    assert_eq!((said(0, Some(summary(7, 0)), None), said(0, None, Some(&record))), (passed(&[]), passed(&[])), "as does an unreadable record or an exit 0 with no summary");
    assert_eq!(said(3, None, None), Verdict::Failed, "a build the tree failed");
    assert_eq!(said(32, Some(summary(4, 1)), None), Verdict::Failed, "a test the summary counts as failed");
    assert_eq!(said(32, Some(summary(0, 0)), None), passed(&[]), "a test run lost to the workers, with no failed test counted, is not a failure of the tree");
    assert_eq!((said(2, Some(summary(5, 0)), Some(&record)), verdict(None, None, None, &owner, &BTreeSet::new(), false)), (passed(&[]), Verdict::Passed(BTreeSet::new())), "any other status is the engine's");
    let failed = summary(10, 1);
    let remote = "Remote Execution Error on Execute with digest 87983734:147 for ReSession GRPC-SESSION-ID";
    let target = "  ✗ root//crates/north-lander:north-lander-test-it";
    let cancelled = [remote, "Error: (Error was returned on the stream by RE: Failed to run command: stream terminated by RST_STREAM with error code: CANCEL)", failed.as_str(), "1 TESTS FAILED", target].map(str::to_owned);
    assert_eq!(remote_error(&cancelled), Some(remote), "a remote worker that cancels an action before any case ran is infrastructure, not a red (north#1115)");
    assert_eq!(verdict(Some(32), Some(&failed), None, &owner, &BTreeSet::new(), remote_error(&cancelled).is_some()), passed(&[]), "that target is left to the declared line, which runs it once more");
    let written = infrastructure_record("a52e364", "root//crates/north-lander:north-lander-test-it", remote, "passed", 7);
    assert_eq!(written[..6], ["ledger", "incident", "record", "--key", "remote-execution-infrastructure:a52e364:7", "--class"], "the attempt is written as an incident of its own class under its own key (north#1115)");
    assert!(written.last().is_some_and(|detail| detail.starts_with("head=a52e364 target=root//crates/north-lander:north-lander-test-it error=Remote Execution Error") && detail.ends_with("outcome=passed")), "the head, target, error text and outcome are the detail: {written:?}");
    let deadline = [remote, "Error: (Error was returned on the stream by RE: Failed to run command: context deadline exceeded)", failed.as_str(), "1 TESTS FAILED", target].map(str::to_owned);
    assert_eq!(remote_error(&deadline), None, "a deadline at the action's own test timeout is a red, not infrastructure (north#1115)");
    assert_eq!(verdict(Some(32), Some(&failed), None, &owner, &BTreeSet::new(), remote_error(&deadline).is_some()), Verdict::Failed, "and it fails the check");
    let named = [remote, "FAIL [   0.154s] (3/9) north-lander tests::rejects_a_stale_lease", failed.as_str(), "1 TESTS FAILED", target].map(str::to_owned);
    assert_eq!(remote_error(&named), None, "a case the runner names as failed ran and failed, so it is never infrastructure and never run again (north#1115)");
    assert_eq!(verdict(Some(32), Some(&failed), None, &owner, &BTreeSet::new(), remote_error(&named).is_some()), Verdict::Failed, "a named red beside a remote execution error stays a red of the tree");
}
