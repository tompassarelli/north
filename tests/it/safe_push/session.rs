use super::{Landing, Run, Scratch};
use std::path::PathBuf;
use std::time::Duration;

fn lane_with_check(scratch: &Scratch, name: &str, check: &str) -> (Landing, PathBuf) {
    let landing = Landing::new(scratch, name);
    let lane = landing.lane();
    landing.configure(&lane, &[], &[check], "a land check under test");
    (landing, lane)
}

#[test]
fn a_hung_land_check_times_out_and_fails_the_push() {
    let scratch = Scratch::new();
    let (_landing, lane) = lane_with_check(&scratch, "hung-check-timeout", "sleep 40");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"]).env("XDG_RUNTIME_DIR", scratch.path().join("runtime")).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "1").start();
    let status = running.exits_within(Duration::from_secs(20));
    let output = running.output();
    if status.is_none() {
        running.kill();
    }
    let status = status.unwrap_or_else(|| panic!("a hung land check held safe-push past its timeout: {output}"));
    assert!(!status.success(), "a timed out land check let the push succeed: {output}");
    assert!(output.contains("ran past its 1 s deadline"), "missing land check timeout: {output}");
}

