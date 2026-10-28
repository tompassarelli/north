#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

const SCOPE_SYSTEMD_RUN: &str = "#!/bin/sh
for argument; do case $argument in --unit=*) unit=${argument#--unit=} ;; esac; done
while [ \"$1\" != -- ]; do shift; done
shift
read host _ </proc/self/stat; echo \"$$ $host\" >\"$XDG_STATE_HOME/scopes/$unit\"
";

pub const SCOPE_SYSTEMCTL: &str = "#!/bin/sh
case $2 in show) unit=$3 ;; *) eval \"unit=\\${$#}\" ;; esac
{ read pid host <\"$XDG_STATE_HOME/scopes/$unit\"; } 2>/dev/null
state=$(sed 's/.*) //' \"/proc/$host/stat\" 2>/dev/null | cut -c1)
live() { [ -n \"$pid\" ] && [ -n \"$state\" ] && [ \"$state\" != Z ]; }
case $2 in
show) if live; then echo active; else echo inactive; fi ;;
kill) live && kill -KILL \"$pid\" ;;
stop) if live; then kill -TERM \"$pid\"; fi ;;
freeze) live && kill -STOP \"$pid\" ;;
thaw) live && kill -CONT \"$pid\" ;;
esac
";

pub fn scope_systemd_run(command: &str) -> String {
    format!("{SCOPE_SYSTEMD_RUN}exec {command}\n")
}

pub struct Wrapper(pub Child);

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Wrapper {
    pub fn exited(&mut self) -> Option<ExitStatus> {
        self.0.try_wait().unwrap()
    }

    pub fn stderr(&mut self) -> String {
        let mut text = String::new();
        self.0.stderr.take().unwrap().read_to_string(&mut text).unwrap();
        text
    }
}

pub struct Host {
    scratch: tempfile::TempDir,
}

impl Host {
    pub fn new(prefix: &str, shims: &[(&str, &str)]) -> Self {
        let scratch = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let bin = scratch.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (name, body) in shims {
            fs::write(bin.join(name), body).unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::create_dir_all(scratch.path().join("agent-capacity-v1")).unwrap();
        fs::create_dir_all(scratch.path().join("state/scopes")).unwrap();
        fs::write(scratch.path().join("agent-capacity-v1/mode"), "away\n").unwrap();
        fs::write(scratch.path().join("pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
        fs::write(scratch.path().join("gpu"), "0\n").unwrap();
        fs::write(scratch.path().join("meminfo"), "MemTotal:       98655232 kB\nMemAvailable:   81920000 kB\n").unwrap();
        fs::write(scratch.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
        Self { scratch }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.scratch.path().join(name)
    }

    pub fn bundle() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/skills/machine-capacity/scripts/machine-capacity.mjs")
    }

    pub fn helper(&self, arguments: &[&str]) -> Command {
        let mut command = self.command("bun");
        command.arg(Self::bundle()).args(arguments);
        command.stderr(self.helper_log());
        command
    }

    pub fn helper_log(&self) -> fs::File {
        fs::OpenOptions::new().create(true).append(true).open(self.path("helper.stderr")).unwrap()
    }

    pub fn helper_stderr(&self) -> String {
        fs::read_to_string(self.path("helper.stderr")).unwrap_or_default()
    }

    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap_or_default());
        for (key, value) in [
            ("PATH", path),
            ("XDG_RUNTIME_DIR", self.path("").display().to_string()),
            ("XDG_STATE_HOME", self.path("state").display().to_string()),
            ("AGENT_CAPACITY_DB", self.path("capacity.db").display().to_string()),
            ("AGENT_CAPACITY_USAGE_LOG", self.path("usage.jsonl").display().to_string()),
            ("AGENT_CAPACITY_CGROUP_ROOT", self.path("cgroup").display().to_string()),
            ("AGENT_CAPACITY_CPU_PRESSURE", self.path("pressure").display().to_string()),
            ("AGENT_CAPACITY_GPU_BUSY", self.path("gpu").display().to_string()),
            ("AGENT_CAPACITY_GPU_CLIENTS", "0".to_owned()),
            ("AGENT_CAPACITY_LOADAVG", self.path("loadavg").display().to_string()),
            ("AGENT_CAPACITY_NVIDIA_MEMORY", self.path("nvidia-memory").display().to_string()),
            ("AGENT_CAPACITY_CORES", "24".to_owned()),
            ("AGENT_CAPACITY_DISK_FREE_GIB", "500".to_owned()),
            ("AGENT_CAPACITY_MEMINFO", self.path("meminfo").display().to_string()),
            ("AGENT_CAPACITY_NORTH", env!("CARGO_BIN_EXE_north").to_owned()),
            ("AGENTS_ORCHESTRATION", self.path("orchestration.toml").display().to_string()),
            ("CAPACITY_WATCHDOG_DIR", self.path("no-watchdog").display().to_string()),
            ("GAME_RESERVE_STATE", self.path("game-reserve.json").display().to_string()),
        ] {
            command.env(key, value);
        }
        command.env_remove("DBUS_SESSION_BUS_ADDRESS");
        command
    }

    fn records(&self, directory: &str) -> Vec<Value> {
        fs::read_dir(self.path(directory)).map_or_else(|_| Vec::new(), |entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
                .filter_map(|entry| fs::read_to_string(entry.path()).ok())
                .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
                .collect()
        })
    }

    pub fn leases(&self, class: &str) -> usize {
        self.records("agent-capacity-v1/leases")
            .iter()
            .filter(|lease| lease["class"] == class || (class == "quiet" && lease["quiet"] == true))
            .count()
    }

    pub fn queued(&self, class: &str) -> usize {
        self.records("agent-capacity-v1/queue").iter().filter(|ticket| ticket["class"] == class).count()
    }

    pub fn status(&self) -> Value {
        let output = self.helper(&["status"]).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn until(&self, what: &str, mut done: impl FnMut(&Self) -> bool) -> Instant {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(self) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}; helper stderr:\n{}", self.helper_stderr());
            std::thread::sleep(Duration::from_millis(5));
        }
        Instant::now()
    }
}
