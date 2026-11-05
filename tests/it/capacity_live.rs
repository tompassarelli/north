use crate::capacity_host;

use capacity_host::{Exclusive, Host};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

const CGROUP: &str = "/sys/fs/cgroup";
const MEMINFO: &str = "MemTotal: 98655232 kB\nMemAvailable: 71680000 kB\n";

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn systemd_user_runtime() -> Option<PathBuf> {
    let uid = fs::metadata("/proc/self").ok()?.uid();
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from(format!("/run/user/{uid}")), PathBuf::from);
    let manager = Path::new(CGROUP).join(format!("user.slice/user-{uid}.slice/user@{uid}.service"));
    let reachable = Command::new("systemctl").args(["--user", "show-environment"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
    (reachable && manager.is_dir() && runtime.join("systemd").is_dir() && Path::new(CGROUP).join("cgroup.controllers").is_file()).then_some(runtime)
}

struct Live {
    host: Host,
    subslice: String,
}

impl Live {
    fn new(prefix: &str) -> Option<Self> {
        let runtime = systemd_user_runtime()?;
        let host = Host::new(prefix, &[]);
        symlink(runtime.join("systemd"), host.path("systemd")).unwrap();
        let _ = fs::remove_file(host.path("agent-capacity-v1/mode"));
        fs::write(host.path("meminfo"), MEMINFO).unwrap();
        let subslice = format!("test{}x{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        Some(Self { host, subslice })
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = self.host.helper(arguments);
        command.env_remove("AGENT_CAPACITY_CGROUP_ROOT").env("AGENT_CAPACITY_SUBSLICE", &self.subslice).env("AGENT_CAPACITY_SETTLE_SECONDS", "3");
        command
    }

    fn wait(&self, what: &str, done: impl FnMut(&Host) -> bool) {
        self.host.until(what, done);
    }

    fn run_until_exit(&self, arguments: &[&str]) -> (Option<i32>, Vec<Value>) {
        let stderr = self.host.path("exit.stderr");
        let mut command = self.command(arguments);
        command.stdout(Stdio::null()).stderr(fs::File::create(&stderr).unwrap());
        let status = command.status_exclusive().unwrap();
        (status.code(), events(&stderr))
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let slices = ["agent-capacity", "agent-batch", "native"].map(|prefix| format!("{prefix}-{}.slice", self.subslice));
        let _ = Command::new("systemctl").args(["--user", "stop"]).args(slices).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

fn events(stderr: &Path) -> Vec<Value> {
    fs::read_to_string(stderr).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

macro_rules! live {
    ($name:literal) => {
        match Live::new($name) {
            Some(live) => live,
            None => {
                eprintln!("skipped: no systemd user manager with delegated cgroup v2");
                return;
            }
        }
    };
}

#[test]
fn a_run_past_its_timeout_has_its_scope_and_every_descendant_killed() {
    let live = live!("capacity-live-timeout-");
    let pid_file = live.host.path("sleeper.pid");
    let script = "sleep 60 & child=$!; printf '%s\\n' \"$child\" >\"$1\"; wait \"$child\"";
    let (code, _) = live.run_until_exit(&["run", "--class", "heavy", "--owner", "fixture:/capacity", "--timeout-seconds", "1", "--", "bash", "-c", script, "fixture-scope", pid_file.to_str().unwrap()]);
    assert_ne!(code, Some(0));
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_owned();
    assert!(pid.parse::<u32>().is_ok_and(|pid| pid > 0), "{pid}");
    live.wait("the descendant to stop", |_| !Path::new(&format!("/proc/{pid}")).exists());
}

