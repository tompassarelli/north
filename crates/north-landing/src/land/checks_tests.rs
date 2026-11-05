use std::path::Path;

use super::{INFRASTRUCTURE, infrastructure, unfit, unfit_host};

#[test]
fn a_crate_the_host_lacks_offline_is_an_unfit_host() {
    let log = "Compiling x\nerror: failed to download `ahash v0.8.12`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\nsafe-push: verdict refused\n";
    assert_eq!(unfit_host(log).as_deref(), Some("error: failed to download `ahash v0.8.12`"));
    assert_eq!(unfit_host("error: failed to download `ahash v0.8.12`\nCaused by: HTTP 502\n"), None, "a download that failed online is not this signature");
}

#[test]
fn north_missing_on_path_is_an_unfit_host() {
    for line in ["bash: line 1: north: command not found", "sh: 1: north: not found", "safe-push: north not on PATH; refusing to land without the language gate; NOT pushed.", "safe-push: north missing on PATH — refusing to push without the dependency gate; NOT pushed."] {
        assert_eq!(unfit_host(&format!("checking\n{line}\nsafe-push: verdict refused\n")).as_deref(), Some(line));
    }
    assert_eq!(unfit_host("bash: line 1: northd: command not found\n"), None);
}

#[test]
fn an_offload_transport_failure_that_exits_255_is_an_unfit_host() {
    let line = "north capacity offload: shipping the lander runtime 93b443a5742a to tom@cx53: rsync failed: File exists (17)";
    assert_eq!(unfit_host(&format!("{line}\nsafe-push: land check check-owners exited 255; NOT pushed.\n")).as_deref(), Some(line));
    assert_eq!(unfit_host(&format!("{line}\nsafe-push: land check check-owners exited 1; NOT pushed.\n")), None, "only the transport's 255 exit names the host");
    assert_eq!(unfit_host("safe-push: land check cargo test exited 255; NOT pushed.\n"), None, "a 255 exit with no transport line is the lane's");
}

#[test]
fn a_lane_failure_is_not_an_unfit_host() {
    assert_eq!(unfit_host("test a::b ... FAILED\nsafe-push: land check cargo test exited 101; NOT pushed.\nsafe-push: verdict refused\n"), None);
}

#[test]
fn an_unfit_host_is_a_transient_infrastructure_verdict_that_names_its_log() {
    let verdict = unfit(Path::new("/logs/check.log"), "bash: line 1: north: command not found\n").unwrap();
    assert_eq!(verdict, super::Verdict::Transient(format!("{INFRASTRUCTURE}bash: line 1: north: command not found; log: /logs/check.log")));
    let super::Verdict::Transient(detail) = verdict else { unreachable!() };
    assert!(infrastructure(&detail));
    assert!(!infrastructure("build slot moved"));
}
