use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process, kill_process_group, test_kill_process};

use crate::error::{NorthError, NorthResult};
use crate::platform::session_scope;

use super::detached::{self, Prior};
use super::model::ProviderName;
use super::runtime::{HostSpec, binary, new_uuid};

const SOCKET_WAIT: Duration = Duration::from_secs(10);
const REPLY_WAIT: Duration = Duration::from_secs(10);
const TERM_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

pub struct Host {
    dir: PathBuf,
    proc: PathBuf,
    cgroups: PathBuf,
}

impl Host {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, proc: "/proc".into(), cgroups: "/sys/fs/cgroup".into() }
    }

    #[cfg(test)]
    fn reading(mut self, proc: PathBuf, cgroups: PathBuf) -> Self {
        self.proc = proc;
        self.cgroups = cgroups;
        self
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
            self.retire()?;
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

    /// Stops the previous host's own process tree and leaves its scope holding the runs the session
    /// detached, recorded beside `host.scope` for the new host to read. A host that is already gone
    /// left no tree to tell from those runs, so its whole scope is stopped.
    fn retire(&self) -> NorthResult<()> {
        let unit = fs::read_to_string(self.dir.join("host.scope")).unwrap_or_default();
        let tree = self.pid().filter(|pid| alive(*pid)).map(|pid| (pid, detached::host_tree(&self.proc, &self.cgroups, pid)));
        let Some((host, tree)) = tree.filter(|(_, tree)| !tree.is_empty()) else { return self.terminate() };
        let pids: Vec<Pid> = tree.iter().filter_map(|pid| i32::try_from(*pid).ok().and_then(Pid::from_raw)).collect();
        for pid in &pids {
            let _ = kill_process(*pid, Signal::Term);
        }
        let deadline = Instant::now() + TERM_GRACE;
        while tree.iter().any(|pid| alive(*pid)) && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        for pid in pids {
            let _ = kill_process(pid, Signal::Kill);
        }
        Prior { unit: unit.trim().to_owned(), host }.remember(&self.dir)?;
        let _ = fs::remove_file(self.dir.join("host.scope"));
        Ok(())
    }

    fn stop_priors(&self) -> NorthResult<()> {
        for prior in Prior::all(&self.dir) {
            let stopped = Command::new(binary("systemctl")).args(["--user", "stop", &prior.unit]).stdin(Stdio::null()).stderr(Stdio::piped()).output()?;
            if !stopped.status.success() && stopped.status.code() != Some(5) {
                return Err(NorthError::Refused(format!("could not stop prior session scope {}: {}", prior.unit, String::from_utf8_lossy(&stopped.stderr).trim())));
            }
        }
        Prior::forget_all(&self.dir);
        Ok(())
    }

    pub fn terminate(&self) -> NorthResult<()> {
        self.stop_priors()?;
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

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt;
    use std::os::unix::fs::symlink;
    use std::process::Child;

    use super::super::detached::{Procfs, Scope, ended, ended_message};
    use super::*;

    const SLICE: &str = "user.slice/agent.slice";

    fn sleeper(seconds: &str) -> Child {
        Command::new("sleep").arg(seconds).process_group(0).stdin(Stdio::null()).spawn().unwrap()
    }

    fn enroll(proc: &Path, pid: u32, unit: &str) {
        let dir = proc.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        for file in ["stat", "cmdline"] {
            symlink(format!("/proc/{pid}/{file}"), dir.join(file)).unwrap();
        }
        fs::write(dir.join("cgroup"), format!("0::/{SLICE}/{unit}\n")).unwrap();
        let procs = proc.parent().unwrap().join("cgroup").join(SLICE).join(unit);
        fs::create_dir_all(&procs).unwrap();
        let mut members = fs::read_to_string(procs.join("cgroup.procs")).unwrap_or_default();
        members.push_str(&format!("{pid}\n"));
        fs::write(procs.join("cgroup.procs"), members).unwrap();
    }

    #[test]
    fn a_revival_stops_the_previous_hosts_tree_but_not_a_run_it_detached_and_the_new_host_reads_that_scope() {
        let root = tempfile::tempdir().unwrap();
        let (proc, cgroups, dir) = (root.path().join("proc"), root.path().join("cgroup"), root.path().join("session"));
        fs::create_dir_all(&dir).unwrap();
        let mut old_host = Command::new("sh").args(["-c", "sleep 300 & wait"]).process_group(0).stdin(Stdio::null()).spawn().unwrap();
        let mut run = sleeper("301");
        let deadline = Instant::now() + Duration::from_secs(5);
        let child = loop {
            let children = fs::read_to_string(format!("/proc/{0}/task/{0}/children", old_host.id())).unwrap_or_default();
            if let Some(child) = children.split_whitespace().next().and_then(|pid| pid.parse::<u32>().ok()) {
                break child;
            }
            assert!(Instant::now() < deadline, "the old host never started its child");
            std::thread::sleep(POLL);
        };
        for pid in [old_host.id(), child, run.id()] {
            enroll(&proc, pid, "north-session-old.scope");
        }
        enroll(&proc, std::process::id(), "north-session-new.scope");
        fs::write(dir.join("host.scope"), "north-session-old.scope").unwrap();
        fs::write(dir.join("host.pid"), old_host.id().to_string()).unwrap();

        Host::new(dir.clone()).reading(proc.clone(), cgroups.clone()).retire().unwrap();
        let _ = old_host.wait();

        assert!(!alive(old_host.id()) && !alive(child), "the previous host's tree outlived the revival");
        assert!(alive(run.id()), "the revival killed the run the session detached");
        assert!(!dir.join("host.scope").exists());
        let revived = Procfs::new(proc, cgroups, std::process::id()).after(Prior::all(&dir));
        let before = revived.detached();
        assert_eq!(before.iter().map(|run| run.pid).collect::<Vec<_>>(), [run.id()], "the new host counts the prior scope's run: {before:?}");
        run.kill().unwrap();
        run.wait().unwrap();
        let ended = ended(&before, &revived.detached());
        assert!(ended_message(&ended).contains(&format!("pid {}", before[0].pid)), "{ended:?}");
        assert_eq!(ended.len(), 1);
    }
}
