use super::*;
use north_foundation::test_inputs::git::{git_in, init_repo};

const WORKSPACE: &str = "[[package]]\nname = \"north\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://example.invalid/index\"\nchecksum = \"aa\"\n";

fn lane_with_lock(lock: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    init_repo(directory.path(), None);
    std::fs::write(directory.path().join("Cargo.lock"), WORKSPACE).unwrap();
    git_in(directory.path(), &["add", "Cargo.lock"]);
    git_in(directory.path(), &["commit", "-qm", "lock"]);
    git_in(directory.path(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(directory.path().join("Cargo.lock"), lock).unwrap();
    directory
}

#[test]
fn a_confined_run_of_a_bumped_lane_goes_through_depbox_and_a_failed_replacement_reports_why() {
    let through_depbox: fn(&mut Command, &str) -> u8 = |command, _| if command.get_args().eq(["depbox", "run", "--", "target/debug/north"]) { 42 } else { 7 };
    let run_in_lane = |lock: &str| {
        let lane = lane_with_lock(lock);
        let arguments = ["--lane".to_owned(), lane.path().display().to_string(), "--".to_owned(), "target/debug/north".to_owned()];
        run_with(&arguments, true, |_| Some(9), through_depbox)
    };
    assert_eq!(run_in_lane(&WORKSPACE.replace("checksum = \"aa\"", "checksum = \"cc\"")), 42);
    assert_eq!(run_in_lane(WORKSPACE), 9);
    assert_eq!(replace_with(&mut Command::new("/nonexistent/tool"), "tool"), 127);
    assert_eq!(replace_with(&mut Command::new("/"), "tool"), 126);
}
