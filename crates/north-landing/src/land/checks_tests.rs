use super::{named, unfit_host};

#[test]
fn a_crate_the_host_lacks_offline_is_an_unfit_host_only_when_main_fails_it_too_and_a_refusal_is_named_by_a_gate_that_failed_not_one_that_passed() {
    let missing = "Compiling x\nerror: failed to download `ahash v0.8.12`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n";
    let main_too = format!("{missing}safe-push: land check cargo clippy exited 101 on this lane and also fails on base tree 0123456789ab, so the failure is the check environment\n");
    assert_eq!(unfit_host(&main_too).as_deref(), Some("error: failed to download `ahash v0.8.12`"));
    assert_eq!(unfit_host(&format!("{missing}safe-push: land check failed; rerunning it on base tree 0123456789ab: cargo clippy\nsafe-push: verdict refused\n")), None, "main passed the rerun, so the crate is the lane's");
    assert_eq!(unfit_host("error: failed to download `ahash v0.8.12`\nCaused by: HTTP 502\n"), None, "a download that failed online is not this signature");

    let passed = "language-gate: passed\nbuild-cache-gate: passed\nstrategy-replay: refused the landing\nsafe-push: verdict refused\n";
    assert_eq!(named(passed, "/logs/candidate.log".to_owned()), "/logs/candidate.log");
    let refused = "language-gate: passed\ndep-gate: Cargo.toml: x: added dependency needs a reason\nsafe-push: verdict refused\n";
    assert_eq!(named(refused, "/logs/candidate.log".to_owned()), "dep-gate; log: /logs/candidate.log");
}
