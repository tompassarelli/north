use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn live() -> bool {
    north().args(["10s", "--", "true"]).output().is_ok_and(|probe| probe.status.success())
}

fn north() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command.arg("run-bounded");
    command
}

fn bounded(arguments: &[&str]) -> Output {
    north().args(arguments).output().unwrap()
}

#[test]
fn run_bounded_rejects_a_duration_outside_one_second_to_twenty_four_hours_and_a_missing_command_with_usage_status_two() {
    for arguments in [&["0s", "--", "true"][..], &["25h", "--", "true"], &["30m", "true"]] {
        let output = bounded(arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: north run-bounded"));
    }
}

#[test]
fn run_bounded_keeps_the_commands_exit_status_and_scrubs_systemd_own_environment_names() {
    if !live() {
        eprintln!("skipped: no systemd user manager with the owner runtime");
        return;
    }
    assert_eq!(bounded(&["10s", "--", "bash", "-c", "exit 7"]).status.code(), Some(7));
    let output = north().args(["10s", "--", "env"]).env("NOTIFY_SOCKET", "/x").env("LISTEN_FDS", "3").env("RUN_BOUNDED_KEEP", "1").output().unwrap();
    let environment = String::from_utf8_lossy(&output.stdout);
    assert!(environment.lines().any(|line| line == "RUN_BOUNDED_KEEP=1"), "{environment}");
    assert!(!environment.lines().any(|line| line.starts_with("NOTIFY_SOCKET=") || line.starts_with("LISTEN_FDS=")), "{environment}");
}

#[test]
fn run_bounded_reaps_the_timed_out_subtree_and_leaves_root_owned_files_root_owned() {
    if !live() {
        eprintln!("skipped: no systemd user manager with the owner runtime");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let pid_file = scratch.path().join("host-pid");
    let script = "(awk '/^NSpid:/ {print $2}' /proc/self/status >\"$1\"; exec sleep 30) & wait";
    let status = north().args(["1s", "--", "bash", "-c", script, "bash"]).arg(&pid_file).status().unwrap();
    assert!(!status.success());
    let child = fs::read_to_string(&pid_file).unwrap();
    let alive = Path::new("/proc").join(child.trim());
    let begun = Instant::now();
    while alive.exists() && begun.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive.exists(), "the timed subtree outlived the bound");
    let owner = bounded(&["10s", "--", "stat", "-c", "%U", "/nix/store"]);
    assert_eq!(String::from_utf8_lossy(&owner.stdout).trim(), "root");
}

#[test]
fn run_bounded_reclaims_a_capacity_lease_when_its_wrapper_is_killed() {
    if !live() {
        eprintln!("skipped: no systemd user manager with the owner runtime");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let user_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", rustix::process::getuid().as_raw()));
    fs::create_dir_all(root.join("runtime/agent-capacity-v1")).unwrap();
    std::os::unix::fs::symlink(format!("{user_runtime}/systemd"), root.join("runtime/systemd")).unwrap();
    let _ = fs::copy(format!("{user_runtime}/agent-capacity-v1/mode"), root.join("runtime/agent-capacity-v1/mode"));
    fs::write(root.join("cpu.pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    fs::write(root.join("gpu-busy"), "0\n").unwrap();
    fn capacity<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
        command
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AGENT_CAPACITY_CPU_PRESSURE", root.join("cpu.pressure"))
            .env("AGENT_CAPACITY_GPU_BUSY", root.join("gpu-busy"))
            .env("AGENT_CAPACITY_GPU_CLIENTS", "0")
            .env("CAPACITY_WATCHDOG_DIR", root.join("no-watchdog"))
            .env("AGENT_CAPACITY_DB", root.join("capacity.db"))
            .env("AGENT_CAPACITY_USAGE_LOG", root.join("usage.jsonl"))
    }
    let holding = || {
        let mut status = Command::new(env!("CARGO_BIN_EXE_north"));
        let output = capacity(status.args(["capacity", "host", "status"]), root).output().unwrap();
        serde_json::from_slice::<serde_json::Value>(&output.stdout).ok().and_then(|value| value["holding"].as_array().map(Vec::len)).unwrap_or(usize::MAX)
    };
    let mut wrapper = north();
    let mut child = capacity(wrapper.args(["1m", "--", env!("CARGO_BIN_EXE_north"), "capacity", "host", "run", "--owner", "dead-wrapper", "--class", "moderate", "--timeout-seconds", "60", "--", "sleep", "60"]), root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let begun = Instant::now();
    while holding() != 1 && begun.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(holding(), 1);
    child.kill().unwrap();
    child.wait().unwrap();
    let begun = Instant::now();
    while holding() != 0 && begun.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(holding(), 0);
}
