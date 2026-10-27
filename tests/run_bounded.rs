use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread::sleep;
use std::time::Duration;

const SYSTEMD_RUN: &str = "/run/current-system/sw/bin/systemd-run";

fn bounded(arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command.arg("run-bounded").args(arguments);
    command
}

fn has_user_manager() -> bool {
    Path::new(SYSTEMD_RUN).is_file()
        && Command::new("/run/current-system/sw/bin/systemctl")
            .args(["--user", "is-system-running"])
            .output()
            .is_ok_and(|output| matches!(String::from_utf8_lossy(&output.stdout).trim(), "running" | "degraded"))
}

fn usage_status(arguments: &[&str]) -> Option<i32> {
    bounded(arguments).output().unwrap().status.code()
}

#[test]
fn a_duration_outside_one_second_to_a_day_or_a_missing_separator_is_a_usage_error() {
    assert_eq!(usage_status(&["0s", "--", "true"]), Some(2));
    assert_eq!(usage_status(&["25h", "--", "true"]), Some(2));
    assert_eq!(usage_status(&["30m", "true"]), Some(2));
    assert_eq!(usage_status(&["30m", "--"]), Some(2));
    assert_eq!(usage_status(&[]), Some(2));
}

#[test]
fn the_command_exit_status_comes_back_and_its_whole_subtree_is_reaped_when_the_limit_ends() {
    if !has_user_manager() {
        return;
    }
    assert_eq!(bounded(&["10s", "--", "bash", "-c", "exit 7"]).status().unwrap().code(), Some(7));

    let scratch = tempfile::tempdir().unwrap();
    let pid_file = scratch.path().join("host-pid");
    let status = bounded(&[
        "1s",
        "--",
        "bash",
        "-c",
        "(awk '/^NSpid:/ {print $2}' /proc/self/status >\"$1\"; exec sleep 30) &\nwait",
        "bash",
    ])
    .arg(&pid_file)
    .status()
    .unwrap();
    assert_ne!(status.code(), Some(0));
    let child = fs::read_to_string(&pid_file).unwrap();
    let proc = PathBuf::from("/proc").join(child.trim());
    for _ in 0..100 {
        if !proc.exists() {
            break;
        }
        sleep(Duration::from_millis(20));
    }
    assert!(!proc.exists(), "the timed subtree outlived its limit");
}

#[test]
fn files_the_command_creates_stay_root_owned_and_systemd_socket_variables_are_not_passed_on() {
    if !has_user_manager() {
        return;
    }
    let owner = bounded(&["10s", "--", "stat", "-c", "%U", "/nix/store"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&owner.stdout).trim(), "root");

    let environment = bounded(&["10s", "--", "env"]).env("NOTIFY_SOCKET", "/x").env("LISTEN_FDS", "3").output().unwrap();
    let listing = String::from_utf8_lossy(&environment.stdout);
    assert!(!listing.lines().any(|line| line.starts_with("NOTIFY_SOCKET=") || line.starts_with("LISTEN_FDS=")), "{listing}");
}

struct Capacity {
    scratch: tempfile::TempDir,
    script: PathBuf,
}

impl Capacity {
    fn prepare() -> Option<Self> {
        let scratch = tempfile::tempdir().unwrap();
        let script = scratch.path().join("machine-capacity.mjs");
        let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/skills/machine-capacity/scripts/build-machine-capacity");
        if Command::new("bun").arg("--version").output().is_err() || !Command::new(build).arg(&script).output().ok()?.status.success() {
            return None;
        }
        let root = scratch.path();
        let real = PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", rustix::process::getuid().as_raw())));
        fs::create_dir_all(root.join("runtime/agent-capacity-v1")).unwrap();
        symlink(real.join("systemd"), root.join("runtime/systemd")).unwrap();
        let _ = fs::copy(real.join("agent-capacity-v1/mode"), root.join("runtime/agent-capacity-v1/mode"));
        fs::write(root.join("cpu.pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
        fs::write(root.join("gpu-busy"), "0\n").unwrap();
        Some(Self { scratch, script })
    }

    fn command(&self, program: &str) -> Command {
        let root = self.scratch.path();
        let mut command = Command::new(program);
        command
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AGENT_CAPACITY_CPU_PRESSURE", root.join("cpu.pressure"))
            .env("AGENT_CAPACITY_GPU_BUSY", root.join("gpu-busy"))
            .env("AGENT_CAPACITY_GPU_CLIENTS", "0")
            .env("CAPACITY_WATCHDOG_DIR", root.join("no-watchdog"))
            .env("AGENT_CAPACITY_DB", root.join("capacity.db"))
            .env("AGENT_CAPACITY_USAGE_LOG", root.join("usage.jsonl"));
        command
    }

    fn run_through_bounded(&self, owner: &str, command: &[&str]) -> Command {
        let mut run = self.command(env!("CARGO_BIN_EXE_north"));
        run.args(["run-bounded", "1m", "--", "bun"]).arg(&self.script).args(["run", "--owner", owner, "--class", "moderate", "--timeout-seconds", "60", "--"]).args(command);
        run
    }

    fn holding(&self) -> usize {
        let output: Output = self.command("bun").arg(&self.script).arg("status").output().unwrap();
        let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        status["holding"].as_array().map_or(0, Vec::len)
    }

    fn wait_for_holding(&self, count: usize, tries: usize) {
        for _ in 0..tries {
            if self.holding() == count {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        assert_eq!(self.holding(), count);
    }
}

#[test]
fn machine_capacity_reaches_the_user_bus_inside_the_bounded_scope_and_a_killed_wrapper_releases_its_lease() {
    if !has_user_manager() {
        return;
    }
    let Some(capacity) = Capacity::prepare() else {
        panic!("bun or build-machine-capacity failed on a host that has a systemd user manager");
    };
    let admitted = capacity.run_through_bounded("bus-test", &["true"]).output().unwrap();
    let combined = [admitted.stdout, admitted.stderr].concat();
    assert!(String::from_utf8_lossy(&combined).contains("RELEASED"), "{}", String::from_utf8_lossy(&combined));

    let mut wrapper: Child = capacity
        .run_through_bounded("dead-wrapper", &["sleep", "60"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    capacity.wait_for_holding(1, 100);
    wrapper.kill().unwrap();
    let _ = wrapper.wait();
    capacity.wait_for_holding(0, 50);
}
