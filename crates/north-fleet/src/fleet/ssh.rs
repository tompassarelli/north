use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::platform::process::try_run_command_within;

use super::link::quote;

pub(crate) const SSH_WAIT: Duration = Duration::from_secs(20);

pub(crate) fn over_ssh_via(ssh: &Path, host: &str, words: &[&str], input: Option<&[u8]>, wait: Duration) -> NorthResult<Vec<u8>> {
    let command = format!("north {}", words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
    let mut ssh_command = Command::new(ssh);
    ssh_command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, "--", &command]);
    match try_run_command_within(ssh_command, input, wait) {
        Ok(Some(output)) if output.status.success() => Ok(output.stdout),
        Ok(Some(output)) => Err(NorthError::Usage(format!("ssh {host} {command}: {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        Ok(None) => Err(NorthError::Usage(format!("ssh {host} {command}: no answer within {}s", wait.as_secs()))),
        Err(error) => Err(NorthError::Usage(format!("ssh {host} {command}: could not start {}: {error}", ssh.display()))),
    }
}

