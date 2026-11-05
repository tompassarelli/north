use crate::capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use std::fs;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT_SECONDS: u64 = 2;

fn small_host(available_mib: u64) -> Host {
    let host = Host::new("capacity-queue-timeout-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("meminfo"), format!("MemTotal:       {} kB\nMemAvailable:   {} kB\n", 7943 * 1024, available_mib * 1024)).unwrap();
    host
}

fn heavy_build(host: &Host) -> (ExitStatus, String, Duration) {
    let timeout = TIMEOUT_SECONDS.to_string();
    let started = Instant::now();
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command.args(["capacity", "host", "run", "--class", "heavy", "--owner", "rust-port", "--timeout-seconds", &timeout, "--", "true"]);
    let mut run = Wrapper(command.env_remove("SAFE_PUSH_LAND_HEAD").stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap());
    let mut status = None;
    host.until("the heavy build's exit", |_| {
        status = run.exited();
        status.is_some()
    });
    (status.unwrap(), run.stderr(), started.elapsed())
}

#[test]
fn a_queued_run_exits_at_its_timeout_seconds_naming_the_queue_reason() {
    let (status, stderr, elapsed) = heavy_build(&small_host(1500));
    assert_eq!(status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("\"QUEUED\"") && stderr.contains("past its --timeout-seconds, never admitted: DEFER_MEMORY_HEADROOM"), "{stderr}");
    assert!(elapsed >= Duration::from_secs(TIMEOUT_SECONDS) && elapsed < Duration::from_secs(TIMEOUT_SECONDS + 5), "exited after {elapsed:?}: {stderr}");
}

#[test]
fn an_unmeasured_heavy_build_on_an_idle_8_gib_host_is_admitted_instead_of_queued() {
    let (status, stderr, _) = heavy_build(&small_host(6038));
    assert!(status.success(), "{stderr}");
    assert!(stderr.contains("\"RESERVED\"") && !stderr.contains("\"QUEUED\""), "{stderr}");
}
