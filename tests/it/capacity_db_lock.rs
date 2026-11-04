use crate::capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::process::Output;
use std::time::{Duration, Instant};

fn host() -> Host {
    let host = Host::new("capacity-db-lock-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let seeded = host.helper(&["run", "--owner", "fixture:/seed", "--timeout-seconds", "30", "--", "true"]).status().unwrap();
    assert!(seeded.success(), "{}", host.helper_stderr());
    host
}

fn run_while_locked(host: &Host, hold: Duration, wait_seconds: &str) -> (Output, Duration) {
    let writer = rusqlite::Connection::open(host.path("capacity.db")).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(hold);
        writer.execute_batch("ROLLBACK").unwrap();
    });
    let started = Instant::now();
    let mut command = host.helper(&["run", "--owner", "fixture:/contended", "--timeout-seconds", "30", "--", "true"]);
    let output = command.env("AGENT_CAPACITY_DB_LOCK_WAIT_SECONDS", wait_seconds).output().unwrap();
    let elapsed = started.elapsed();
    holder.join().unwrap();
    (output, elapsed)
}

#[test]
fn a_run_admitted_while_another_connection_holds_the_database_write_lock_waits_and_exits_zero() {
    let host = host();
    let (output, elapsed) = run_while_locked(&host, Duration::from_secs(2), "30");
    let stderr = host.helper_stderr();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains("database is locked"), "{stderr}");
    assert!(elapsed >= Duration::from_secs(2), "the run did not wait for the lock: {elapsed:?}");
}

#[test]
fn a_run_waits_out_a_lock_held_longer_than_the_old_five_second_timeout() {
    let host = host();
    let (output, elapsed) = run_while_locked(&host, Duration::from_secs(7), "30");
    let stderr = host.helper_stderr();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(elapsed >= Duration::from_secs(7), "{elapsed:?}");
}

#[test]
fn a_lock_held_past_the_wait_bound_exits_75_which_private_check_reports_as_not_run() {
    let host = host();
    let (output, _) = run_while_locked(&host, Duration::from_secs(4), "1");
    let stderr = host.helper_stderr();
    assert_eq!(output.status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("database is locked"), "{stderr}");
}
