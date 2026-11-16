//! A fleet host, reached by running a shell script there: this host through `sh -c`, any other through the real
//! `ssh`, never north's ssh shim, which routes a fleet session's ssh through the runtime.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SSH_OPTIONS: [&str; 8] = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3"];
const SHIM_MARK: &[u8] = b"north ssh shim";

pub struct Host {
    pub name: String,
    ssh: Option<PathBuf>,
}

impl Host {
    pub fn fleet(listed: &[String]) -> Result<Vec<Self>, String> {
        let here = here();
        let mut hosts = vec![Self { name: here.clone(), ssh: None }];
        let others: Vec<&String> = listed.iter().filter(|name| **name != here).collect();
        if others.is_empty() {
            return Ok(hosts);
        }
        let ssh = real_ssh().ok_or("no ssh other than north's shim is on PATH, so no other host can be reached")?;
        for name in others {
            if !hosts.iter().any(|host| host.name == *name) {
                hosts.push(Self { name: name.clone(), ssh: Some(ssh.clone()) });
            }
        }
        Ok(hosts)
    }

    pub fn is_here(&self) -> bool {
        self.ssh.is_none()
    }

    pub fn run(&self, script: &str) -> Result<String, String> {
        let mut command = match &self.ssh {
            None => {
                let mut command = Command::new("sh");
                command.arg("-c").arg(script);
                command
            }
            Some(ssh) => {
                let mut command = Command::new(ssh);
                command.args(SSH_OPTIONS).arg("--").arg(&self.name).arg(script);
                command
            }
        };
        let output = command.stdin(Stdio::null()).output().map_err(|error| format!("{}: {error}", self.name))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        match output.status.success() {
            true => Ok(stdout),
            false => Err(format!("{}: {} ({})", self.name, String::from_utf8_lossy(&output.stderr).trim(), output.status)),
        }
    }
}

pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

pub fn here() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_default()
}

fn real_ssh() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).map(|dir| dir.join("ssh")).find(|candidate| is_executable(candidate) && !is_shim(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn is_shim(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    fs::File::open(path).and_then(|file| file.take(256).read_to_end(&mut head)).is_ok_and(|_| head.starts_with(b"#!") && head.windows(SHIM_MARK.len()).any(|window| window == SHIM_MARK))
}

pub fn each<T: Send>(hosts: &[Host], work: impl Fn(&Host) -> T + Sync) -> Vec<T> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = hosts.iter().map(|host| scope.spawn(|| work(host))).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-@:".contains(&byte))
}
