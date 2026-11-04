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

#[cfg(test)]
mod tests {
    use super::over_ssh_via;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    fn script(dir: &std::path::Path, name: &str, body: &str, mode: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn a_program_that_cannot_start_reports_the_spawn_error_not_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = script(dir.path(), "ssh", "#!/bin/sh\n", 0o644);
        let message = over_ssh_via(&ssh, "host", &["fleet", "list"], None, Duration::from_secs(5)).unwrap_err().to_string();
        assert!(message.contains("could not start"), "{message}");
        assert!(message.contains("os error 13"), "{message}");
        assert!(!message.contains("no answer within"), "{message}");
    }

    #[test]
    fn a_program_that_outlasts_its_wait_reports_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = script(dir.path(), "ssh", "#!/bin/sh\nexec sleep 30\n", 0o755);
        let message = over_ssh_via(&ssh, "host", &["fleet", "list"], None, Duration::from_millis(300)).unwrap_err().to_string();
        assert!(message.contains("no answer within"), "{message}");
    }
}
