use super::{Landing, Run, Scratch, wait_until, write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn lane_with_check(scratch: &Scratch, name: &str, check: &str) -> (Landing, PathBuf) {
    let landing = Landing::new(scratch, name);
    let lane = landing.lane();
    landing.configure(&lane, &[], &[check], "a land check under test");
    (landing, lane)
}

fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid]).stderr(std::process::Stdio::null()).status().unwrap().success()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default().trim().to_owned()
}

#[test]
fn sigint_cancels_a_dry_run_during_its_land_check_with_exit_130_no_false_landed_receipt_and_no_land_check_left_running() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "cancelled-check");
    let lane = landing.lane();
    let pid_file = landing.root.join("check-pid");
    landing.configure(&lane, &[], &[&format!("printf '%s\\n' \"$$\" >'{}'; exec sleep 40", pid_file.display())], "cancel during the land check");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"]).start();
    assert!(wait_until(Duration::from_secs(60), || !read(&pid_file).is_empty()), "the land check never started: {}", running.output());
    assert!(Command::new("kill").args(["-INT", &running.pid().to_string()]).status().unwrap().success());
    let status = running.exits_within(Duration::from_secs(60));
    let output = running.output();
    assert_eq!(status.and_then(|status| status.code()), Some(130), "a dry run cancelled during its land check did not exit 130: {output}");
    assert!(output.lines().any(|line| line == "safe-push: cancelled"), "missing cancelled receipt: {output}");
    assert!(!output.lines().any(|line| line.starts_with("safe-push: landed ")), "a cancelled dry run falsely reported the base as landed: {output}");
    let check = read(&pid_file);
    let gone = wait_until(Duration::from_secs(5), || !alive(&check));
    if !gone {
        let _ = Command::new("kill").arg(&check).status();
    }
    assert!(gone, "a cancelled dry run left its land check running");
}

#[test]
fn a_dead_land_check_with_orphaned_children_releases_safe_push() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "dead-check-orphans");
    let lane = landing.lane();
    let orphan = landing.root.join("orphan-pid");
    landing.configure(&lane, &[], &[&format!("sleep 40 & echo $! >'{}'; exit 7", orphan.display())], "check leaves an orphan holding its output");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"]).start();
    let status = running.exits_within(Duration::from_secs(15));
    let _ = Command::new("kill").arg(read(&orphan)).stderr(std::process::Stdio::null()).status();
    let output = running.output();
    if status.is_none() {
        running.kill();
    }
    let status = status.unwrap_or_else(|| panic!("a dead land check left safe-push waiting on its orphaned log children: {output}"));
    assert!(!status.success(), "a failed land check let the push succeed: {output}");
    assert!(output.contains("exited 7"), "missing land check failure: {output}");
}

#[test]
fn a_hung_land_check_times_out_and_fails_the_push() {
    let scratch = Scratch::new();
    let (_landing, lane) = lane_with_check(&scratch, "hung-check-timeout", "sleep 40");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"]).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").start();
    let status = running.exits_within(Duration::from_secs(20));
    let output = running.output();
    if status.is_none() {
        running.kill();
    }
    let status = status.unwrap_or_else(|| panic!("a hung land check held safe-push past its timeout: {output}"));
    assert!(!status.success(), "a timed out land check let the push succeed: {output}");
    assert!(output.contains("ran past its 2 s deadline"), "missing land check timeout: {output}");
}

#[test]
fn a_frozen_land_check_times_out_only_after_its_timeout_plus_the_time_it_was_frozen() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "frozen-check-timeout");
    let lane = landing.lane();
    let started = landing.root.join("started");
    landing.configure(&lane, &[], &[&format!("touch '{}'; sleep 40", started.display())], "check is frozen past its timeout");
    let runtime = landing.root.join("runtime");
    let lease = runtime.join("agent-capacity-v1/leases/frozen.json");
    write(&lease, "{\"expiresAt\":1000,\"frozenAt\":1000}\n");
    let mut running = Run::new(&scratch, &lane, &["--dry-run", "--to", "main"])
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("AGENT_CAPACITY_LEASE", "frozen")
        .env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2")
        .start();
    let deadline = "ran past its 2 s deadline";
    assert!(wait_until(Duration::from_secs(60), || started.exists()), "the frozen land check never started");
    std::thread::sleep(Duration::from_secs(4));
    assert!(!running.output().contains(deadline), "a frozen land check was ended by its wall-clock timeout: {}", running.output());
    write(&lease, "{\"expiresAt\":5000}\n");
    std::thread::sleep(Duration::from_millis(1600));
    assert!(!running.output().contains(deadline), "a frozen land check ended before its timeout plus the time it was frozen: {}", running.output());
    let ended = wait_until(Duration::from_secs(3), || running.output().contains(deadline));
    let status = running.exits_within(Duration::from_secs(10));
    let output = running.output();
    running.kill();
    assert!(ended, "a thawed land check outlived its timeout plus the time it was frozen: {output}");
    assert!(status.is_some_and(|status| !status.success()), "a timed out frozen land check let the push succeed: {output}");
}
