//! Runs a session's provider CLI on another machine: the session host here runs `north
//! session-link` instead of the CLI, so its socket, stream and transcript stay on this machine
//! while the CLI and its tools run there. The other machine keeps the same home layout, so the
//! working directory and the provider runtime are the same paths on both, while the account is the
//! profile the policy declares for that machine, which the registry resolves before handing the
//! spec here.
//!
//! There the CLI runs as one transient user service per session, held apart from any ssh
//! connection: it reads an input file and appends to an output file in the session's held
//! directory, so a stalled link costs no frame and no background task, and the link reattaches.
//! The service stops, with every process it started, when the CLI exits, when the link stops it,
//! or after twelve hours with no link attached. Starting a session first stops any earlier service
//! of the same session, so a revive never runs two CLIs on one conversation.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

use super::link::{Link, quote};
use super::model::{Machine, SessionRecord};
use super::records::record_file;
use super::runtime::{HostSpec, binary, new_uuid};

const DETACHED: Duration = Duration::from_secs(12 * 60 * 60);
const HELD: &str = ".local/state/north/held";

/// `sh -c HOLD DIR UNIT DETACHED_SECS CLI...`: the service's main process. It feeds the CLI the
/// input file through a FIFO, records the CLI's status and stops the service once no link has been
/// attached for DETACHED_SECS.
const HOLD: &str = r#"d=$0 u=$1 limit=$2; shift 2
mkfifo "$d/feed" || exit 70
{ "$@" < "$d/feed" >> "$d/out" 2>> "$d/err"; echo $? > "$d/status"; } &
cli=$!
{ seen=$(date +%s); while sleep 60; do
  if read -r p b < "$d/attach" && [ "$(cut -d' ' -f22 "/proc/$p/stat")" = "$b" ]; then seen=$(date +%s)
  elif [ $(($(date +%s) - seen)) -ge "$limit" ]; then systemctl --user stop "$u"; fi
done; } 2>/dev/null &
tail -c +1 -f --pid="$cli" "$d/in" > "$d/feed"
wait "$cli""#;

/// `sh -c PRUNE HELD`: removes held directories untouched for three days whose service is gone.
const PRUNE: &str = r#"for h in "$0"/*.service; do
  [ -d "$h" ] && [ -z "$(find "$h" -maxdepth 1 -mmin -4320 -print -quit)" ] && ! systemctl --user -q is-active "${h##*/}" &&
    rm -f "$h/in" "$h/out" "$h/err" "$h/status" "$h/attach" "$h/feed" "$h/nonce" && rmdir "$h"
done"#;

/// The session's host spec as it runs: here, or over ssh on the machine its record names.
pub fn placed(spec: HostSpec, record: &SessionRecord) -> NorthResult<HostSpec> {
    match &record.machine {
        Some(_) if spec.shared => Err(NorthError::Usage(format!("{} runs one shared host per account, so it cannot run on another machine", record.provider.label()))),
        Some(machine) => over_ssh(spec, machine),
        None => Ok(spec),
    }
}

pub fn over_ssh(spec: HostSpec, machine: &Machine) -> NorthResult<HostSpec> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or_else(|| NorthError::State("HOME is not set".into()))?;
    held_under(spec, machine, Path::new(&home))
}

pub(super) fn held_under(spec: HostSpec, machine: &Machine, home: &Path) -> NorthResult<HostSpec> {
    let held = home.join(HELD);
    let unit = unit(&spec.key);
    let dir = held.join(&unit).to_string_lossy().into_owned();
    let nonce = new_uuid()?;
    let (q_unit, q_dir) = (quote(&unit), quote(&dir));
    let mut launch = format!(
        "sh -c {} {}; systemctl --user stop {q_unit} 2>/dev/null; mkdir -p {q_dir} && cd {q_dir} && rm -f in out err status attach feed nonce && : > in && : > out && : > err && printf %s {} > nonce && cd {} && systemd-run --user --quiet --collect --same-dir --unit={q_unit} --property=KillMode=control-group \"--setenv=PATH=$PATH\"",
        quote(PRUNE),
        quote(&held.to_string_lossy()),
        quote(&nonce),
        quote(&spec.cwd.to_string_lossy())
    );
    for (key, value) in &spec.env {
        launch.push(' ');
        launch.push_str(&quote(&format!("--setenv={key}={}", value.to_string_lossy())));
    }
    launch.push_str(&format!(" -- sh -c {} {q_dir} {q_unit} {}", quote(HOLD), DETACHED.as_secs()));
    for arg in &spec.argv {
        launch.push(' ');
        launch.push_str(&quote(arg));
    }
    let link = Link { ssh: binary("ssh"), destination: machine.ssh.clone(), unit, dir, nonce, launch };
    let argv = [vec![std::env::current_exe()?.display().to_string(), "session-link".into()], link.arguments()].concat();
    Ok(HostSpec { argv, ..spec })
}

pub fn leave_record(record: &SessionRecord, placed: &Path) {
    if let Err(error) = mirror_through(&binary("ssh"), record, placed) {
        eprintln!("north fleet: {}'s record is not on its machine, so that machine's run ledger cannot place its transcript: {}", record.name, error.user_message());
    }
}

fn mirror_through(ssh: &Path, record: &SessionRecord, placed: &Path) -> NorthResult<()> {
    let (Some(machine), Some(conversation)) = (&record.machine, &record.conversation) else { return Ok(()) };
    let directory = placed.join(conversation);
    let file = record_file(&directory).to_string_lossy().into_owned();
    let script = format!("mkdir -p {} && cat > {} && mv {} {}", quote(&directory.to_string_lossy()), quote(&format!("{file}.tmp")), quote(&format!("{file}.tmp")), quote(&file));
    let mut child = Command::new(ssh)
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &machine.ssh, "--", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().ok_or_else(|| NorthError::State("ssh has no input".into()))?.write_all(&serde_json::to_vec_pretty(record)?)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(NorthError::Rejected(format!("{} refused the record: {}", machine.name, String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_placed_sessions_record_lands_on_its_machine_under_its_conversation() {
        let scratch = tempfile::tempdir().unwrap();
        let ssh = scratch.path().join("ssh");
        fs::write(&ssh, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/argv\"\nwhile [ \"$1\" != -- ]; do shift; done\nexec sh -c \"$2\"\n").unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
        let record: SessionRecord = serde_json::from_value(serde_json::json!({
            "name": "sc-459", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null,
            "conversation": "it's-a-conversation", "task_ref": "smashcraft#459", "machine": {"name": "far", "ssh": "tom@far"}
        }))
        .unwrap();
        let placed = scratch.path().join("fleet/.placed");
        mirror_through(&ssh, &record, &placed).unwrap();
        assert_eq!(super::super::records::read_record(&placed.join("it's-a-conversation")).unwrap(), record);
        assert!(fs::read_to_string(scratch.path().join("argv")).unwrap().lines().any(|line| line == "tom@far"));
        mirror_through(&ssh, &SessionRecord { machine: None, ..record }, &scratch.path().join("here")).unwrap();
        assert!(!scratch.path().join("here").exists(), "a session on this machine needs no copy");
    }
}
