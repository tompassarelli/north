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

use north_session_host::held::{HOLD, PRUNE, unit};

use crate::error::{NorthError, NorthResult};
use crate::platform::process::try_run_command_within;

use super::link::{Link, quote};
use super::model::{Account, Machine, SessionRecord};
use super::records::record_file;
use super::runtime::{HostSpec, Runtime, Seen, binary, new_uuid};
use super::ssh::SSH_WAIT;

const DETACHED: Duration = Duration::from_secs(12 * 60 * 60);
const HELD: &str = ".local/state/north/held";

/// The session's host spec as it runs: here, or over ssh on the machine its record names.
pub fn placed(spec: HostSpec, record: &SessionRecord) -> NorthResult<HostSpec> {
    match &record.machine {
        Some(_) if spec.shared => Err(NorthError::Usage(format!("{} runs one shared host per account, so it cannot run on another machine", record.provider.label()))),
        Some(machine) => over_ssh(spec, machine),
        None => Ok(spec),
    }
}

/// Why `account`'s profile would start a session without hooks on `machine`, or here when it is `None`; a machine that does not answer is left to the launch itself to report.
pub fn unguarded_where_it_runs(runtime: &dyn Runtime, account: &Account, machine: Option<&Machine>) -> Option<String> {
    let Some(machine) = machine else { return runtime.unguarded(account, &|path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text)) };
    let read = |path: &Path| {
        let mut ssh = Command::new(binary("ssh"));
        ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &machine.ssh, "--", &format!("cat -- {}", quote(&path.to_string_lossy()))]);
        match try_run_command_within(ssh, None, SSH_WAIT).ok().flatten() {
            Some(output) if output.status.success() => Seen::Text(String::from_utf8_lossy(&output.stdout).into_owned()),
            Some(output) if output.status.code() == Some(1) => Seen::Absent,
            _ => Seen::Unreachable,
        }
    };
    runtime.unguarded(account, &read).map(|reason| format!("{reason} on {}", machine.name))
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
        "sh -c {} {}; systemctl --user stop {q_unit} 2>/dev/null; mkdir -p {q_dir} && cd {q_dir} && rm -f in out err status attach feed nonce && : > in && : > out && : > err && printf %s {} > nonce && cd {} && systemd-run --user --quiet --collect --same-dir --slice=agent.slice --unit={q_unit} --property=KillMode=control-group \"--setenv=PATH=$PATH\"",
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
    Ok(HostSpec { argv, cwd: home.to_path_buf(), ..spec })
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

/// Copies `conversation` between two accounts' homes where the session runs: on `machine` over ssh, else here.
pub fn carry_conversation(runtime: &dyn Runtime, machine: Option<&Machine>, from: &Account, to: &Account, conversation: &str) -> NorthResult<()> {
    let Some(script) = runtime.carry_script(from, to, conversation) else { return Ok(()) };
    let (mut command, place) = match machine {
        Some(machine) => {
            let mut ssh = Command::new(binary("ssh"));
            ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &machine.ssh, "--", &script]);
            (ssh, machine.name.as_str())
        }
        None => {
            let mut sh = Command::new("sh");
            sh.args(["-c", &script]);
            (sh, "here")
        }
    };
    command.stdin(Stdio::null());
    match try_run_command_within(command, None, SSH_WAIT) {
        Ok(Some(output)) if output.status.success() => Ok(()),
        Ok(Some(output)) => Err(NorthError::State(format!("carrying {conversation} from account {} to {} {place}: {}: {}", from.label, to.label, output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        Ok(None) => Err(NorthError::State(format!("carrying {conversation} from account {} to {} {place}: no answer within {}s", from.label, to.label, SSH_WAIT.as_secs()))),
        Err(error) => Err(NorthError::State(format!("carrying {conversation} {place}: {error}"))),
    }
}
