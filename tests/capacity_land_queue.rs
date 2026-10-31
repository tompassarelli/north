mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use std::fs;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

const LIMIT_SECONDS: u64 = 1;

fn north(host: &Host, arguments: &[&str]) -> Wrapper {
    Wrapper(host.command(env!("CARGO_BIN_EXE_north")).args(arguments).env_remove("SAFE_PUSH_LAND_HEAD").stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap())
}

/// A land check under its deadline whose exclusive capacity run waits behind a heavy lease for longer than the deadline, then runs `command`.
fn land_check_queued_past_its_limit(command: &[&str]) -> ExitStatus {
    let host = Host::new("capacity-land-queue-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let go = host.path("go");
    let hold = format!("until [ -e {} ]; do sleep 0.05; done", go.display());
    let _blocker = north(&host, &["capacity", "host", "run", "--class", "heavy", "--owner", "fixture:/blocker", "--timeout-seconds", "60", "--", "sh", "-c", &hold]);
    host.until("the blocking heavy lease", |host| host.leases("heavy") == 1);
    let lease = host.path("agent-capacity-v1/leases/.json").display().to_string();
    let limit = LIMIT_SECONDS.to_string();
    let inner = [&["capacity", "deadline", &lease, &limit, "--", env!("CARGO_BIN_EXE_north"), "capacity", "host", "run", "--class", "exclusive", "--owner", "land-check", "--timeout-seconds", "60", "--"][..], command].concat();
    let mut landing = north(&host, &inner);
    host.until("the land check's queue ticket", |host| host.queued("exclusive") == 1);
    std::thread::sleep(Duration::from_secs(LIMIT_SECONDS * 2));
    assert!(landing.exited().is_none(), "the land check died while queued: {}", landing.stderr());
    fs::write(&go, "").unwrap();
    let mut status = None;
    host.until("the land check's end", |_| {
        status = landing.exited();
        status.is_some()
    });
    status.unwrap()
}

#[test]
fn a_land_check_queued_longer_than_its_limit_but_running_within_it_passes() {
    assert!(land_check_queued_past_its_limit(&["true"]).success());
}

#[test]
fn a_land_check_running_past_its_limit_is_still_stopped() {
    assert_eq!(land_check_queued_past_its_limit(&["sleep", "30"]).code(), Some(124));
}
