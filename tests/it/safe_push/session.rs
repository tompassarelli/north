use super::{Landing, Run, Scratch, wait_until, write};
use std::time::Duration;

#[test]
fn a_hung_land_check_times_out_and_fails_the_push() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "hung-check-timeout");
    let lane = landing.lane();
    let started = landing.root.join("started");
    let finished = landing.root.join("finished");
    landing.configure(&lane, &[], &[&format!("touch '{}'; sleep 40; touch '{}'", started.display(), finished.display())], "check hangs past its timeout");
    let clock = landing.root.join("clock");
    write(&clock.join("now"), "0");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"])
        .env("XDG_RUNTIME_DIR", landing.root.join("runtime"))
        .env("NORTH_CAPACITY_DEADLINE_CLOCK", &clock)
        .env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "1")
        .start();
    assert!(wait_until(Duration::from_secs(60), || started.exists()), "the hung land check never started: {}", running.output());
    write(&clock.join("now"), "2000");
    let ended = wait_until(Duration::from_secs(60), || running.output().contains("ran past its 1 s deadline"));
    let status = running.exits_within(Duration::from_secs(60));
    let output = running.output();
    running.kill();
    assert!(ended, "a hung land check outlived its timeout: {output}");
    assert!(status.is_some_and(|status| !status.success()), "a timed out land check let the push succeed: {output}");
    assert!(!finished.exists(), "a hung land check was left running past its deadline: {output}");
}
