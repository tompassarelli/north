use std::io;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::platform::process::run_command_within;

const READ_LIMIT: Duration = Duration::from_secs(600);

pub(super) trait Remote {
    fn command(&self, script: &str) -> Command;

    fn output(&self, script: &str) -> io::Result<Vec<u8>> {
        let output = run_command_within(self.command(script), None, READ_LIMIT)
            .ok_or_else(|| io::Error::other("remote command timed out or did not start"))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(io::Error::other(format!("remote command failed: {}", String::from_utf8_lossy(&output.stderr).trim())))
        }
    }

    fn stream(&self, script: &str) -> io::Result<Child> {
        self.command(script).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()
    }
}

pub(super) struct Ssh {
    pub(super) host: String,
}

impl Remote for Ssh {
    fn command(&self, script: &str) -> Command {
        let mut command = Command::new("ssh");
        command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", &self.host, "--", script]);
        command
    }
}
