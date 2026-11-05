use super::{RUN, TRANSPORT_FAILED, ensure_shipped, quote};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const SKEWED_CURRENT: &str = "echo \"remote-current refuses: $*\" >&2\nexit 2\n";
const LANDER_NORTH: &str = "if [ \"$1\" = capacity ]; then shift 5; exec \"$@\"; fi\nif [ \"$1\" = dev-shell ]; then shift 5; exec \"$@\"; fi\necho \"lander $*\"\n";
const LANDER_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn build_host(with_lander_runtime: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/bin")).unwrap();
    crate::foundation::shim::write_script(&home.path().join(".local/bin/north"), SKEWED_CURRENT);
    if with_lander_runtime {
        let runtime = home.path().join(".local/state/north/runtimes").join(LANDER_SHA);
        fs::create_dir_all(&runtime).unwrap();
        crate::foundation::shim::write_script(&runtime.join("north"), LANDER_NORTH);
    }
    home
}

fn run_on(home: &Path, runtime: &str) -> Output {
    let slot = tempfile::tempdir().unwrap();
    let script = RUN.replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string());
    let head = format!("set -eu\nslot={} command='north check' limit=10 base=b head=h landing= landref= runtime={}\n{script}", quote(&slot.path().display().to_string()), quote(runtime));
    Command::new("sh").args(["-c", &head]).env("HOME", home).output().unwrap()
}

#[test]
fn every_north_the_offloaded_command_and_its_wrapper_call_is_the_landers_runtime_not_the_build_hosts_newer_current() {
    let home = build_host(true);
    let skewed = run_on(home.path(), "");
    assert_eq!(skewed.status.code(), Some(2), "without the pin the build host's current is what runs");
    assert!(String::from_utf8_lossy(&skewed.stderr).contains("remote-current refuses: capacity deadline"));
    let pinned = run_on(home.path(), LANDER_SHA);
    assert!(pinned.status.success(), "{}", String::from_utf8_lossy(&pinned.stderr));
    assert_eq!(String::from_utf8_lossy(&pinned.stdout), "lander check\n");
    assert!(!String::from_utf8_lossy(&pinned.stderr).contains("remote-current"));
}

#[test]
fn a_lander_runtime_missing_on_the_build_host_stops_the_run_as_a_transport_failure_before_any_check_runs() {
    let home = build_host(false);
    let missing = run_on(home.path(), LANDER_SHA);
    assert_eq!(missing.status.code(), Some(i32::from(TRANSPORT_FAILED)));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains(&format!("the lander runtime {LANDER_SHA} is not on this host")));
}

#[test]
fn the_remote_script_receives_the_landers_runtime_as_its_tenth_word() {
    assert!(super::REMOTE.contains("runtime=${10:-}"));
}

#[test]
fn a_runtime_absent_from_the_build_host_is_shipped_before_the_check_and_a_present_one_is_not_shipped_again() {
    let events = std::cell::RefCell::new(Vec::new());
    let note = |event| events.borrow_mut().push(event);
    let shipped = ensure_shipped("gex45", LANDER_SHA, || { note("probe"); false }, || { note("ship"); Ok(()) });
    assert_eq!((shipped, events.take()), (Ok(()), vec!["probe", "ship"]));
    let present = ensure_shipped("gex45", LANDER_SHA, || { note("probe"); true }, || { note("ship"); Ok(()) });
    assert_eq!((present, events.take()), (Ok(()), vec!["probe"]));
}

#[test]
fn a_failed_shipment_names_the_runtime_and_host_so_the_check_never_starts() {
    let refused = ensure_shipped("gex45", LANDER_SHA, || false, || Err("rsync failed".to_owned()));
    assert_eq!(refused, Err(format!("shipping the lander runtime {LANDER_SHA} to gex45: rsync failed")));
}
