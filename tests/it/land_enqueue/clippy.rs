use std::fs;

use super::{Fixture, git, north_enqueue_with, written};

fn lane_with_clippy(fixture: &Fixture, clean: bool) -> std::path::PathBuf {
    let runs = fixture.root.join("clippy-runs");
    let command = format!("echo ran >>{} && test -e clippy-clean # cargo clippy --all-targets -- -D warnings", runs.display());
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\tafter = \"** :: {command}\"\n")).unwrap();
    if clean {
        fs::write(fixture.repo.join("clippy-clean"), "clean\n").unwrap();
    }
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check"]);
    runs
}

fn runs(path: &std::path::Path) -> usize {
    fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

#[test]
fn enqueue_refuses_a_lane_failing_clippy_on_its_changed_crates_and_reuses_a_pass_for_the_same_tree() {
    let refused = Fixture::new();
    let counter = lane_with_clippy(&refused, false);
    let (output, _) = north_enqueue_with(&refused, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("a check before queueing exited") && stderr.contains("to rerun clippy alone: cargo clippy --all-targets --workspace -- -D warnings"), "{stderr}");
    assert_eq!(runs(&counter), 1, "{stderr}");
    assert!(written(&refused.state).iter().all(|path| path.extension().is_none_or(|extension| extension != "json")), "a refused lane writes no ticket: {:?}", written(&refused.state));

    let passed = Fixture::new();
    let counter = lane_with_clippy(&passed, true);
    let (first, _) = north_enqueue_with(&passed, &[]);
    let first_log = format!("{}{}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    assert!(!first_log.contains("a check before queueing exited"), "{first_log}");
    assert_eq!(runs(&counter), 1, "{first_log}");
    let (second, _) = north_enqueue_with(&passed, &[]);
    let second_log = format!("{}{}", String::from_utf8_lossy(&second.stdout), String::from_utf8_lossy(&second.stderr));
    assert!(second_log.contains("reused from the verdict cache"), "{second_log}");
    assert_eq!(runs(&counter), 1, "a cached pass for the same tree runs clippy no second time: {second_log}");
    enqueue_queues_a_lane_whose_check_before_queueing_reached_no_verdict_because_no_build_host_answered();
    enqueue_never_compiles_on_the_queueing_host_when_no_build_host_has_room();
}

fn enqueue_queues_a_lane_whose_check_before_queueing_reached_no_verdict_because_no_build_host_answered() {
    let fixture = Fixture::new();
    let command = "exit 75 # cargo clippy --all-targets -- -D warnings";
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\tafter = \"** :: {command}\"\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check no host can run"]);
    let (output, _) = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "{log}");
    assert!(log.contains("reached no verdict") && log.contains("queueing the lane"), "{log}");
    assert!(written(&fixture.state).iter().any(|path| path.extension().is_some_and(|extension| extension == "json")), "{:?}", written(&fixture.state));
}

fn enqueue_never_compiles_on_the_queueing_host_when_no_build_host_has_room() {
    let fixture = Fixture::new();
    let counter = fixture.root.join("clippy-runs");
    let hostless = fixture.root.join("orchestration.toml");
    fs::write(&hostless, "").unwrap();
    let command = format!("echo ran >>{} # cargo clippy --all-targets -- -D warnings", counter.display());
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\trunner = build-host\n\tafter = \"** :: {command}\"\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check that runs on a build host"]);
    let (output, _) = north_enqueue_with(&fixture, &[("AGENTS_ORCHESTRATION", &hostless)]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "{log}");
    assert!(log.contains("no build host has room") && log.contains("queueing the lane"), "{log}");
    assert_eq!(runs(&counter), 0, "{log}");
}
