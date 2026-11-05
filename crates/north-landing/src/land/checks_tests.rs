use std::path::Path;

use super::{INFRASTRUCTURE, Launch, Verdict, failing_check, infrastructure, start, unfit, unfit_host};

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
    let dropped = "safe-push: land check cargo check --tests exited 255; log: /tmp/l.log; NOT pushed.";
    assert_eq!(unfit_host(&format!("safe-push: offloading to cx53\noffload: running abc in /home/tom/.cache/land-offload/code-north-main\n{dropped}\n")).as_deref(), Some(dropped), "an offloaded check whose ssh dropped names its 255 exit");
    assert_eq!(unfit_host("safe-push: offloading to cx53\nsafe-push: land check cargo check exited 101; NOT pushed.\n"), None, "an offloaded check that ran and failed is the lane's");
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

#[test]
fn a_refusal_names_the_land_check_or_gate_that_failed() {
    assert_eq!(failing_check("safe-push: land check: cargo test (deadline 300 s)\ntest a ... FAILED\nsafe-push: land check cargo test exited 101; log: /tmp/a; NOT pushed.\nsafe-push: land check cargo test exited 101 on the base tree too; log: /tmp/b\n").as_deref(), Some("land check cargo test exited 101"));
    assert_eq!(failing_check("safe-push: land check bun test hung: it ran past its 600 s deadline on this lane, while base tree abc finished it in 60 s; log: /tmp/a; NOT pushed.\n").as_deref(), Some("land check bun test hung past its deadline"));
    assert_eq!(failing_check("language-gate: bin/new.py: Python is refused\n").as_deref(), Some("language-gate"));
    assert_eq!(failing_check("safe-push: land check failed; rerunning it on base tree abc: cargo test\nsafe-push: verdict refused\n"), None);
}

fn refused_by(script: &str) -> Verdict {
    let directory = tempfile::tempdir().unwrap();
    let checker = ["bash".to_owned(), "-c".to_owned(), script.to_owned()];
    let launch = Launch { checker: &checker, offload_owner: "test", round: "1".to_owned() };
    let log = directory.path().join("candidate.log");
    match start(&launch, directory.path(), &"a".repeat(40), &log, false) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    }
}

#[test]
fn a_refused_candidate_verdict_names_its_failing_check_before_its_log() {
    let verdict = refused_by("echo 'safe-push: land check cargo clippy exited 101; log: /tmp/l; NOT pushed.'; echo 'safe-push: verdict refused /tmp/tmp.abc'; exit 1");
    assert_eq!(verdict, Verdict::Refused("land check cargo clippy exited 101; log: /tmp/tmp.abc".to_owned()));
    assert_eq!(refused_by("echo 'safe-push: verdict refused E0063'; exit 1"), Verdict::Refused("E0063".to_owned()), "a refusal with no named check keeps its detail");
}
