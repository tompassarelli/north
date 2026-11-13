use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The login a placed session runs as, and how this host reaches it: over ssh on another machine, or, for `USER@.host` as systemd names this host, that user here, whose sessions start only through the north-agent-starts unit and its fixed entry, so no script of this process runs as it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Login<'a> {
    Ssh(&'a str),
    Local(&'a str),
}

impl<'a> Login<'a> {
    /// `None` for a destination ssh would read as an option.
    pub fn of(destination: &'a str) -> Option<Self> {
        match destination.strip_suffix("@.host") {
            _ if destination.is_empty() || destination.starts_with('-') => None,
            Some(user) if !user.is_empty() && !user.contains(['@', '/']) => Some(Self::Local(user)),
            _ => Some(Self::Ssh(destination)),
        }
    }

    /// A command that runs `script` with `sh` as this login through `ssh`, its standard streams this process's, giving up on a connection after `connect`; `None` for a local login, which runs no script: `systemd-run --machine=USER@.host --user` opens PID 1's namespaces and fails for a caller that is not root, and polkit admits the host shell as the agent only for the starter unit.
    pub fn command(self, ssh: &Path, script: &str, connect: Duration) -> Option<Command> {
        let Self::Ssh(destination) = self else { return None };
        let mut command = Command::new(ssh);
        command.args(["-T", "-o", "BatchMode=yes", "-o"]).arg(format!("ConnectTimeout={}", connect.as_secs().max(1))).args([destination, "--", script]);
        Some(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_far_login_runs_its_script_over_ssh_and_a_local_agent_login_runs_none() {
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join("ssh");
        std::fs::write(&ssh, "#!/bin/sh\n[ \"$1 $2 $3 $4 $5\" = \"-T -o BatchMode=yes -o ConnectTimeout=10\" ] || exit 9\nprintf 'ssh %s ' \"$6\"; [ \"$7\" = -- ] || exit 8\nexec sh -c \"$8\"\n").unwrap();
        std::fs::set_permissions(&ssh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let ran = |destination: &str| {
            let output = Login::of(destination).unwrap().command(&ssh, "read line; echo \"$line\" ran", Duration::from_secs(10)).unwrap().stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut child| {
                std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"the script\n")?;
                child.wait_with_output()
            });
            let output = output.unwrap();
            (output.status.code(), String::from_utf8_lossy(&output.stdout).into_owned())
        };
        assert_eq!(ran("agent@cpx62"), (Some(0), "ssh agent@cpx62 the script ran\n".into()));
        assert_eq!(ran("@.host"), (Some(0), "ssh @.host the script ran\n".into()));
        assert_eq!(ran("a@b@.host"), (Some(0), "ssh a@b@.host the script ran\n".into()));
        assert!(Login::of("agent@.host").unwrap().command(&ssh, "id", Duration::from_secs(10)).is_none());
        assert_eq!([Login::of("-oProxyCommand=sh"), Login::of("-x@.host"), Login::of("")], [None, None, None]);
    }
}
