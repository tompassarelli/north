use super::harness::*;
use north_core::foundation::shim;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

const RUNTIMES: [&str; 3] = ["8917932b11111111111111111111111111111111", "cb929a45e8222222222222222222222222222222", "9a896708b01683bd4523100a6c79fa0562e34aac"];

fn spawn(command: &mut Command) -> Child {
    loop {
        match command.spawn() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::yield_now(),
            spawned => break spawned.unwrap(),
        }
    }
}

fn promote(host: &Host, sha: &str) {
    let link = host.path("state/north/current");
    let _ = fs::remove_file(&link);
    symlink(Path::new("runtimes").join(sha), link).unwrap();
}

#[test]
fn a_fleet_watchdog_moves_onto_each_promoted_runtime_in_place_and_keeps_watching_its_session() {
    let host = Host::new();
    for sha in RUNTIMES {
        host.built(sha);
        shim::copy_executable(env!("CARGO_BIN_EXE_north"), host.runtime(sha).join("north"));
    }
    promote(&host, RUNTIMES[0]);
    let mut session_host = spawn(Command::new("sleep").arg("600"));
    let session = host.path("state/north/fleet/worker-1");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "worker-1", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": session_host.id(), "role": "worker"});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let north = |sha: &str, args: &[&str]| {
        let mut command = Command::new(host.runtime(sha).join("north"));
        command.args(args).env("HOME", host.path("home")).env("XDG_STATE_HOME", host.path("state")).env("THREADS_DB", host.path("threads.db")).env_remove("NORTH_RUNTIME_LINK").env_remove("NORTH_FLEET_SESSION");
        command
    };
    let mut watchdog = spawn(north(RUNTIMES[0], &["fleet", "watchdog", "worker-1"]).stdout(std::process::Stdio::null()));
    let pid = watchdog.id();
    let alive = |watchdog: &mut Child| watchdog.try_wait().unwrap().is_none();
    let lag = || {
        let text = String::from_utf8_lossy(&north(RUNTIMES[2], &["runtime", "lag", "--processes"]).output().unwrap().stdout).into_owned();
        text.lines().find(|line| line.starts_with(&format!("{pid} "))).unwrap_or_default().to_owned()
    };
    for sha in &RUNTIMES[1..] {
        promote(&host, sha);
        let moved = north_core::platform::wait::until(Duration::from_secs(10), || lag().ends_with(&format!("runs {}: current", &sha[..10])) || !alive(&mut watchdog));
        assert!(moved && alive(&mut watchdog), "10 s after current moved to {sha}, runtime lag --processes reads the watchdog as {:?}", lag());
    }
    session_host.kill().unwrap();
    session_host.wait().unwrap();
    assert!(north_core::platform::wait::until(Duration::from_secs(20), || !alive(&mut watchdog)), "the moved watchdog never acted on its session host's death");
}
