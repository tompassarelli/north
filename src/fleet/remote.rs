//! Runs a session's provider CLI on another machine: the session host here spawns `ssh` instead of
//! the CLI, so its socket, stream and transcript stay on this machine while the CLI and its tools run
//! there. The other machine keeps the same home layout, so the working directory and the provider
//! runtime are the same paths on both, while the account is the profile the policy declares for that
//! machine, which the registry resolves before handing the spec here.
//!
//! There the CLI runs as one transient user service per session that relays the ssh input to it.
//! When the connection closes, or the CLI exits, the service stops and kills every process it
//! started, background tasks included, which a CLI waiting on them would otherwise keep alive.
//! Starting a session first stops any earlier service of the same session, so a revive never runs
//! two CLIs on one conversation.
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

use super::model::{Machine, SessionRecord};
use super::runtime::{HostSpec, binary};

const RELAY: &str = r#"{ cat; systemctl --user stop --no-block "$0"; } | { "$@"; systemctl --user stop --no-block "$0"; }"#;

/// The session's host spec as it runs: here, or over ssh on the machine its record names.
pub fn placed(spec: HostSpec, record: &SessionRecord) -> NorthResult<HostSpec> {
    match &record.machine {
        Some(_) if spec.shared => Err(NorthError::Usage(format!("{} runs one shared host per account, so it cannot run on another machine", record.provider.label()))),
        Some(machine) => Ok(over_ssh(spec, machine)),
        None => Ok(spec),
    }
}

pub fn over_ssh(spec: HostSpec, machine: &Machine) -> HostSpec {
    let unit = quote(&unit(&spec.key));
    let mut command = format!(
        "systemctl --user stop {unit} 2>/dev/null; cd {} && exec systemd-run --user --quiet --collect --pipe --wait --same-dir --unit={unit} --property=KillMode=control-group \"--setenv=PATH=$PATH\"",
        quote(&spec.cwd.to_string_lossy())
    );
    for (key, value) in &spec.env {
        command.push(' ');
        command.push_str(&quote(&format!("--setenv={key}={}", value.to_string_lossy())));
    }
    command.push_str(&format!(" -- sh -c {} {unit}", quote(RELAY)));
    for arg in &spec.argv {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    let argv = [
        binary("ssh").display().to_string(),
        "-T".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ServerAliveInterval=30".into(),
        "-o".into(),
        "ServerAliveCountMax=4".into(),
        machine.ssh.clone(),
        "--".into(),
        command,
    ];
    HostSpec { argv: argv.into(), ..spec }
}

/// The session's unit, named for reading and suffixed with a digest of the launching host and the
/// exact session name, so neither sanitised look-alikes nor another launching host share it.
fn unit(key: &str) -> String {
    let name: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let launcher = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let digest = Sha256::digest(format!("{}\0{key}", launcher.trim()).as_bytes());
    let suffix: String = digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
    format!("north-fleet-{name}-{suffix}.service")
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    #[test]
    fn the_remote_service_receives_every_argument_and_variable_verbatim() {
        let shims = tempfile::tempdir().unwrap();
        let shim = |name: &str, body: &str| {
            let path = shims.path().join(name);
            fs::write(&path, body).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        };
        shim("systemctl", "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$SHIMS/stopped\"\n");
        shim(
            "systemd-run",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SHIMS/run\"\nset -- \"$@\" --end\nwhile [ \"$1\" != -- ]; do case $1 in --setenv=*) export \"${1#--setenv=}\";; esac; shift; done\nshift\nwhile [ \"$1\" != --end ]; do set -- \"$@\" \"$1\"; shift; done\nshift\nexec \"$@\"\n",
        );
        let tricky = "it's \"quoted\" $HOME `id` \\ ; && newline\nend";
        let spec = HostSpec {
            key: "box one".into(),
            shared: false,
            argv: vec!["sh".into(), "-c".into(), r#"printf '%s|%s|%s' "$PWD" "$NORTH_REMOTE_PROBE" "$1""#.into(), "sh".into(), tricky.into()],
            env: vec![("NORTH_REMOTE_PROBE".into(), tricky.into())],
            env_remove: Vec::new(),
            cwd: PathBuf::from("/"),
        };
        let remote = over_ssh(spec, &Machine { name: "far".into(), ssh: "tom@far".into() });
        assert_eq!(&remote.argv[remote.argv.len() - 3..remote.argv.len() - 1], ["tom@far", "--"]);
        let output = Command::new("sh")
            .args(["-c", remote.argv.last().unwrap()])
            .env("SHIMS", shims.path())
            .env("PATH", format!("{}:{}", shims.path().display(), std::env::var("PATH").unwrap()))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("/|{tricky}|{tricky}"));
        let unit = unit("box one");
        let stopped = fs::read_to_string(shims.path().join("stopped")).unwrap();
        let stops: Vec<&str> = stopped.lines().collect();
        assert_eq!(stops[0], format!("--user stop {unit}"), "an earlier service of the session stops first");
        assert_eq!(stops[1..], [format!("--user stop --no-block {unit}"), format!("--user stop --no-block {unit}")], "end of input and the CLI's exit each stop the service");
        let run = fs::read_to_string(shims.path().join("run")).unwrap();
        assert!(run.lines().any(|line| line == format!("--unit={unit}")), "{run}");
        assert!(run.lines().any(|line| line == "--property=KillMode=control-group"), "{run}");
    }
}
