use crate::capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Stdio;

fn runtime(host: &Host, name: &str) -> PathBuf {
    let directory = host.path("runtimes").join(name);
    fs::create_dir_all(&directory).unwrap();
    let binary = directory.join("north");
    if fs::hard_link(env!("CARGO_BIN_EXE_north"), &binary).is_err() {
        fs::copy(env!("CARGO_BIN_EXE_north"), &binary).unwrap();
    }
    binary
}

fn point_current(host: &Host, binary: &Path) {
    let state = host.path("home/.local/state/north");
    fs::create_dir_all(&state).unwrap();
    let staged = state.join("current.next");
    let _ = fs::remove_file(&staged);
    symlink(binary.parent().unwrap(), &staged).unwrap();
    fs::rename(staged, state.join("current")).unwrap();
}

fn set_pressure(host: &Host, reading: &str) {
    let staged = host.path("pressure.next");
    fs::write(&staged, reading).unwrap();
    fs::rename(staged, host.path("pressure")).unwrap();
}

fn ticket(host: &Host) -> Option<(String, u64)> {
    let entry = fs::read_dir(host.path("agent-capacity-v1/queue")).ok()?.flatten().find(|entry| entry.file_name().to_string_lossy().ends_with(".json"))?;
    let ticket: serde_json::Value = serde_json::from_str(&fs::read_to_string(entry.path()).ok()?).ok()?;
    Some((entry.file_name().to_string_lossy().into_owned(), ticket["wrapperPid"].as_u64()?))
}

#[test]
fn a_queued_wrapper_follows_a_runtime_update_keeping_its_ticket_and_is_then_admitted() {
    let host = Host::new("capacity-runtime-follow-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let (old, new) = (runtime(&host, "old"), runtime(&host, "new"));
    point_current(&host, &old);
    let mut command = host.command(&old);
    command
        .args(["capacity", "host", "run", "--class", "heavy", "--owner", "fixture:/follow", "--timeout-seconds", "60", "--", "true"])
        .env("HOME", host.path("home"))
        .env_remove("AGENT_CAPACITY_NORTH")
        .env_remove("NORTH_BUILD_OFFLOADED")
        .env_remove("SAFE_PUSH_LAND_HEAD")
        .stdout(Stdio::null())
        .stderr(host.helper_log());
    let mut wrapper = Wrapper(command.spawn().unwrap());
    host.until("the heavy run's queue ticket", |host| ticket(host).is_some());
    let (name, pid) = ticket(&host).unwrap();
    assert_eq!(pid, u64::from(wrapper.0.id()));

    point_current(&host, &new);
    host.until("the wrapper to exec the new runtime", |_| fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| exe == new.canonicalize().unwrap()));
    assert_eq!(ticket(&host), Some((name, pid)), "the wrapper kept its ticket across the exec");

    set_pressure(&host, "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    host.until("the wrapper to be admitted and finish", |_| wrapper.0.try_wait().unwrap().is_some());
    assert!(wrapper.0.try_wait().unwrap().unwrap().success(), "{}", host.helper_stderr());
    assert_eq!(ticket(&host), None);
}
