use crate::capacity_host;

use capacity_host::{DrivenClock, Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use std::fs;
use std::process::{ExitStatus, Stdio};

const LIMIT_SECONDS: u64 = 1;

fn north(host: &Host, clock: Option<&DrivenClock>, arguments: &[&str]) -> Wrapper {
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command.args(arguments).env_remove("SAFE_PUSH_LAND_HEAD").stdout(Stdio::null()).stderr(Stdio::piped());
    if let Some(clock) = clock {
        clock.drive(&mut command);
    }
    Wrapper(command.spawn().unwrap())
}

/// A land check under its deadline whose exclusive capacity run waits behind a heavy lease for longer than the deadline, then runs `command`; `running` sees the check once its command holds the lease.
fn land_check_queued_past_its_limit(command: &[&str], running: impl FnOnce(&Host, &DrivenClock)) -> ExitStatus {
    let host = Host::new("capacity-land-queue-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let clock = DrivenClock::new(&host);
    let go = host.path("go");
    let hold = format!("until [ -e {} ]; do sleep 0.05; done", go.display());
    let blocker = [&["capacity", "host", "run", "--class", "heavy", "--owner", "fixture:/blocker", "--timeout-seconds", "60", "--"][..], &["sh", "-c", &hold]].concat();
    let _blocker = north(&host, None, &blocker);
    host.until("the blocking heavy lease", |host| host.leases("heavy") == 1);
    let limit = LIMIT_SECONDS.to_string();
    let inner = [&["capacity", "deadline", "-", &limit, "--", env!("CARGO_BIN_EXE_north"), "capacity", "host", "run", "--class", "exclusive", "--owner", "land-check", "--timeout-seconds", "60", "--"][..], command].concat();
    let mut landing = north(&host, Some(&clock), &inner);
    host.until("the land check's queue ticket", |host| host.queued("exclusive") == 1);
    clock.advance(&host, 1, 1);
    clock.advance(&host, LIMIT_SECONDS * 2 * 1000, 1);
    assert!(landing.exited().is_none(), "the land check died while queued: {}", landing.stderr());
    fs::write(&go, "").unwrap();
    running(&host, &clock);
    let mut status = None;
    host.until("the land check's end", |_| {
        status = landing.exited();
        status.is_some()
    });
    status.unwrap()
}

#[test]
fn a_land_check_queued_longer_than_its_limit_but_running_within_it_passes() {
    assert!(land_check_queued_past_its_limit(&["true"], |_, _| {}).success());
}

#[test]
fn a_land_check_running_past_its_limit_is_still_stopped() {
    let status = land_check_queued_past_its_limit(&["sleep", "30"], |host, clock| {
        host.until("the land check's command to hold the lease", |host| host.queued("exclusive") == 0 && host.leases("exclusive") == 1);
        clock.advance(host, LIMIT_SECONDS * 2 * 1000 + 1, 1);
        clock.set(LIMIT_SECONDS * 10 * 1000);
    });
    assert_eq!(status.code(), Some(124));
}
