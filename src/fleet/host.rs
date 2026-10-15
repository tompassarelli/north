use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group, test_kill_process};

use crate::error::{NorthError, NorthResult};

use super::model::ProviderName;
use super::runtime::{HostSpec, binary};

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
        let socket = self.socket();
        let _ = fs::remove_file(&socket);
        let mut command = Command::new(binary("setsid"));
        command
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
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        let pid = command.spawn()?.id();
        fs::write(self.dir.join("host.pid"), pid.to_string())?;
        self.await_socket(pid)?;
        Ok(pid)
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

    pub fn terminate(&self) {
        let Some(pid) = self.pid() else { return };
        let Some(group) = i32::try_from(pid).ok().and_then(Pid::from_raw) else { return };
        let _ = kill_process_group(group, Signal::Term);
        let deadline = Instant::now() + TERM_GRACE;
        while group_alive(group) && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let _ = kill_process_group(group, Signal::Kill);
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
