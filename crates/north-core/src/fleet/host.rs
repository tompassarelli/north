use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group, test_kill_process};

use crate::error::{NorthError, NorthResult};
use crate::platform::session_scope;

use super::model::ProviderName;
use super::runtime::{HostSpec, binary, new_uuid};

const SOCKET_WAIT: Duration = Duration::from_secs(10);
const REPLY_WAIT: Duration = Duration::from_secs(10);
const TERM_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

pub struct Host {
    dir: PathBuf,
}

impl Host {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn pid(&self) -> Option<u32> {
        fs::read_to_string(self.dir.join("host.pid")).ok()?.trim().parse().ok()
    }

    pub fn running(&self) -> bool {
        self.pid().is_some_and(alive)
    }

    pub fn start(&self, spec: &HostSpec, provider: &ProviderName, idle_exit: Option<Duration>) -> NorthResult<u32> {
        fs::create_dir_all(&self.dir)?;
        if self.dir.join("host.scope").exists() {
            self.terminate()?;
        }
        let socket = self.socket();
        let _ = fs::remove_file(&socket);
        let unit = format!("north-session-{}.scope", new_uuid()?);
        fs::write(self.dir.join("host.scope"), &unit)?;
        let mut command = Command::new(binary("setsid"));
        command
            .arg(binary("systemd-run"))
            .args(["--user", "--scope", "--quiet", "--collect", "--slice=agent.slice", "--property=KillMode=control-group", "--property=TimeoutStopSec=5s", "--unit"])
            .arg(&unit)
            .arg("--")
            .arg(std::env::current_exe()?)
            .args(["session-host", "--name", &spec.key, "--generation", "1", "--state-dir"])
            .arg(&self.dir)
            .arg("--socket")
            .arg(&socket)
            .arg("--idle-exit")
            .arg(idle_exit.map_or(0, |limit| limit.as_secs()).to_string())
            .args(["--provider", provider.label()])
            .arg("--")
            .args(&spec.argv)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = session_scope::login_path(&binary("systemctl")) {
            command.env("PATH", path);
        }
        for key in &spec.env_remove {
            command.env_remove(key);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        command.env("NORTH_FLEET_SCOPE", &unit);
        let _ = fs::remove_file(self.dir.join("host.pid"));
        let pid = command.spawn()?.id();
        if let Err(error) = self.await_socket(pid) {
            self.terminate()?;
            return Err(error);
        }
        self.pid().ok_or_else(|| NorthError::State("session host did not record its pid".into()))
    }

    pub fn send(&self, frames: &[String]) -> NorthResult<()> {
        if frames.is_empty() {
            return Ok(());
        }
        let stream = UnixStream::connect(self.socket())?;
        stream.set_read_timeout(Some(REPLY_WAIT))?;
        let mut writer = &stream;
        let mut replies = BufReader::new(&stream);
        for frame in frames {
            writer.write_all(format!("{frame}\n").as_bytes())?;
            let mut reply = String::new();
            replies.read_line(&mut reply)?;
            if !reply.contains("host_seq") {
                return Err(NorthError::State(format!("session host refused a frame: {}", reply.trim())));
            }
        }
        Ok(())
    }

    pub fn terminate(&self) -> NorthResult<()> {
        let pid = self.pid();
        if let Ok(unit) = fs::read_to_string(self.dir.join("host.scope")) {
            let stopped = Command::new(binary("systemctl")).args(["--user", "stop", unit.trim()]).stdin(Stdio::null()).stderr(Stdio::piped()).output()?;
            if stopped.status.code() == Some(5) {
                return Ok(());
            }
            if !stopped.status.success() {
                return Err(NorthError::Refused(format!("could not stop session scope {}: {}", unit.trim(), String::from_utf8_lossy(&stopped.stderr).trim())));
            }
            return await_exit(pid);
        }
        let Some(pid) = pid else { return Ok(()) };
        let Some(group) = i32::try_from(pid).ok().and_then(Pid::from_raw) else { return Ok(()) };
        let _ = kill_process_group(group, Signal::Term);
        let deadline = Instant::now() + TERM_GRACE;
        while group_alive(group) && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let _ = kill_process_group(group, Signal::Kill);
        Ok(())
    }

    fn socket(&self) -> PathBuf {
        self.dir.join("host.sock")
    }

    fn await_socket(&self, pid: u32) -> NorthResult<()> {
        let deadline = Instant::now() + SOCKET_WAIT;
        while Instant::now() < deadline {
            if UnixStream::connect(self.socket()).is_ok() {
                return Ok(());
            }
            if !alive(pid) {
                return Err(NorthError::State(format!("session host exited; see {}", self.dir.join("g1/stderr.log").display())));
            }
            std::thread::sleep(POLL);
        }
        Err(NorthError::State("session host did not open its socket".into()))
    }
}

fn await_exit(pid: Option<u32>) -> NorthResult<()> {
    let Some(pid) = pid else { return Ok(()) };
    let deadline = Instant::now() + TERM_GRACE;
    while alive(pid) {
        if Instant::now() > deadline {
            return Err(NorthError::Refused(format!("session host {pid} was still running {} s after it was stopped", TERM_GRACE.as_secs())));
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

pub fn alive(pid: u32) -> bool {
    let exists = i32::try_from(pid).ok().and_then(Pid::from_raw).is_some_and(|pid| test_kill_process(pid).is_ok());
    exists && !zombie(pid)
}

fn group_alive(group: Pid) -> bool {
    rustix::process::test_kill_process_group(group).is_ok()
}

fn zombie(pid: u32) -> bool {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).unwrap_or_default();
    stat.rsplit_once(')').is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
}
