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
}
