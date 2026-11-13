use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The login a placed session runs as, and how this host reaches it: over ssh on another machine, or, for `USER@.host` as systemd names this host, in that user's own manager here, where with `--pipe` a dropped caller leaves the unit running, as held launches need.
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

    /// A command that runs `script` with `sh` as this login through `ssh` or `systemd-run` from PATH, its standard streams this process's, giving up on an ssh connection after `connect`.
    pub fn command(self, ssh: &Path, script: &str, connect: Duration) -> Command {
        self.command_with(ssh, Path::new("systemd-run"), script, connect)
    }

    fn command_with(self, ssh: &Path, systemd_run: &Path, script: &str, connect: Duration) -> Command {
        match self {
            Self::Ssh(destination) => {
                let mut command = Command::new(ssh);
                command.args(["-T", "-o", "BatchMode=yes", "-o"]).arg(format!("ConnectTimeout={}", connect.as_secs().max(1))).args([destination, "--", script]);
                command
            }
            Self::Local(user) => {
                let mut command = Command::new(systemd_run);
                command.arg(format!("--machine={user}@.host")).args(["--user", "--pipe", "--wait", "--quiet", "--collect", "--service-type=exec", "--", "sh", "-c", script]);
                command
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_agent_login_runs_its_script_in_the_agent_user_manager_and_a_far_login_over_ssh() {
        let root = tempfile::tempdir().unwrap();
        let (ssh, systemd_run) = (root.path().join("ssh"), root.path().join("systemd-run"));
        let write_script = |path: &Path, body: &str| {
            std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        };
        write_script(&ssh, "[ \"$1 $2 $3 $4 $5\" = \"-T -o BatchMode=yes -o ConnectTimeout=10\" ] || exit 9\nprintf 'ssh %s ' \"$6\"; [ \"$7\" = -- ] || exit 8\nexec sh -c \"$8\"\n");
        write_script(&systemd_run, "[ \"$*\" = \"--machine=agent@.host --user --pipe --wait --quiet --collect --service-type=exec -- sh -c ${11}\" ] || exit 9\nprintf 'manager of agent '\nshift 8; exec \"$@\"\n");
        let ran = |destination: &str| {
            let output = Login::of(destination).unwrap().command_with(&ssh, &systemd_run, "read line; echo \"$line\" ran", Duration::from_secs(10)).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut child| {
                std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"the script\n")?;
                child.wait_with_output()
            });
            let output = output.unwrap();
            (output.status.code(), String::from_utf8_lossy(&output.stdout).into_owned())
        };
        assert_eq!(ran("agent@.host"), (Some(0), "manager of agent the script ran\n".into()));
        assert_eq!(ran("agent@cpx62"), (Some(0), "ssh agent@cpx62 the script ran\n".into()));
        assert_eq!(ran("@.host"), (Some(0), "ssh @.host the script ran\n".into()));
        assert_eq!(ran("a@b@.host"), (Some(0), "ssh a@b@.host the script ran\n".into()));
        assert_eq!([Login::of("-oProxyCommand=sh"), Login::of("-x@.host"), Login::of("")], [None, None, None]);
    }
}
