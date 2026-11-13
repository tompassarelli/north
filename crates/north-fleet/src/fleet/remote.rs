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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use north_session_host::held::{HOLD, PRUNE, unit};

use crate::error::{NorthError, NorthResult};
use crate::platform::process::try_run_command_within;
use crate::platform::session_host_files::{BOUND_CONVERSATION, HELD};

use super::link::{Link, quote};
use super::model::{Account, Machine, SessionRecord};
use super::records::record_file;
use super::runtime::{HostSpec, Runtime, Seen, binary, new_uuid};
use super::ssh::SSH_WAIT;

const DETACHED: Duration = Duration::from_secs(12 * 60 * 60);
const ABSENT: i32 = 3;

/// The session's host spec as it runs: here, or over ssh on the machine its record names.
pub fn placed(spec: HostSpec, record: &SessionRecord, trace: &Path) -> NorthResult<HostSpec> {
    match &record.machine {
        Some(_) if spec.shared => Err(NorthError::Usage(format!("{} runs one shared host per account, so it cannot run on another machine", record.provider.label()))),
        Some(machine) => over_ssh(spec, machine, &record.name, trace),
        None => Ok(spec),
    }
}

/// Whether `account`'s profile would start a session without hooks where it runs, read beside the launch's other work: over ssh to a build host it costs two round trips, which the session host's start and the registration overlap, and the launch takes its verdict before it records the host.
pub struct HooksCheck(JoinHandle<Option<String>>);

impl HooksCheck {
    pub fn start(runtime: Arc<dyn Runtime>, account: &Account, machine: Option<&Machine>) -> Self {
        let (account, machine) = (account.clone(), machine.cloned());
        Self(std::thread::spawn(move || unguarded_where_it_runs(runtime.as_ref(), &account, machine.as_ref())))
    }

    /// Why the session would run without hooks, or `None` when its profile registers them or its machine did not answer, which the launch itself then reports.
    pub fn verdict(self) -> NorthResult<Option<String>> {
        self.0.join().map_err(|_| NorthError::State("the hooks check of a launching session panicked".into()))
    }
}

fn unguarded_where_it_runs(runtime: &dyn Runtime, account: &Account, machine: Option<&Machine>) -> Option<String> {
    let Some(machine) = machine else { return runtime.unguarded(account, &|path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text)) };
    runtime.unguarded(account, &|path| read_on(machine, path)).map(|reason| format!("{reason} on {}", machine.name))
}

/// What `path` holds on `machine`, read through its session login, the user the session runs as there: absent only when no such path exists there, unreachable when the machine does not answer or the file is there but unreadable.
pub fn read_on(machine: &Machine, path: &Path) -> Seen {
    let quoted = quote(&path.to_string_lossy());
    let mut ssh = Command::new(binary("ssh"));
    ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.session_login(), "--", &format!("[ -e {quoted} ] || exit {ABSENT}; cat -- {quoted}")]);
    match try_run_command_within(ssh, None, SSH_WAIT).ok().flatten() {
        Some(output) if output.status.success() => Seen::Text(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(output) if output.status.code() == Some(ABSENT) => Seen::Absent,
        _ => Seen::Unreachable,
    }
}

pub fn over_ssh(spec: HostSpec, machine: &Machine, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or_else(|| NorthError::State("HOME is not set".into()))?;
    held_under(spec, machine, Path::new(&home), session, trace)
}

pub(super) fn held_under(spec: HostSpec, machine: &Machine, home: &Path, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let held = machine.session_home(home).unwrap_or_else(|| home.to_path_buf()).join(HELD);
    let unit = unit(&spec.key);
    let dir = held.join(&unit).to_string_lossy().into_owned();
    let nonce = new_uuid()?;
    let launch = launch_script(&spec, &held, &unit, &dir, &nonce, DETACHED, &[]);
    linked(spec, machine, home, Held { unit, dir, nonce, launch }, session, trace)
}

/// A held unit already running on another machine: its name, its directory and the nonce a link attaches with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Running {
    pub unit: String,
    pub dir: String,
    pub nonce: String,
}

/// The session's host spec when it takes the spare `running` on `machine`: its link attaches to the client already running there instead of launching one.
pub fn attached(spec: HostSpec, machine: &Machine, running: &Running, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or_else(|| NorthError::State("HOME is not set".into()))?;
    let held = Held { unit: running.unit.clone(), dir: running.dir.clone(), nonce: running.nonce.clone(), launch: ":".into() };
    linked(spec, machine, Path::new(&home), held, session, trace)
}

/// The held directory of the unit `unit` under `home`.
pub(super) fn held_dir(home: &Path, unit: &str) -> PathBuf {
    home.join(HELD).join(unit)
}

struct Held {
    unit: String,
    dir: String,
    nonce: String,
    launch: String,
}

fn linked(spec: HostSpec, machine: &Machine, home: &Path, held: Held, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let Held { unit, dir, nonce, launch } = held;
    let link = Link { ssh: binary("ssh"), destination: machine.session_login().to_owned(), unit, dir, nonce, launch, session: session.to_owned(), trace: trace.to_path_buf(), master: None };
    let argv = [vec![std::env::current_exe()?.display().to_string(), "session-link".into()], link.arguments()].concat();
    Ok(HostSpec { argv, cwd: home.to_path_buf(), ..spec })
}

/// The script that starts `spec`'s CLI as the held unit `unit` in `dir`, stopped once no link has been attached for `detached`, with `properties` added to the unit.
pub(super) fn launch_script(spec: &HostSpec, held: &Path, unit: &str, dir: &str, nonce: &str, detached: Duration, properties: &[String]) -> String {
    let (q_unit, q_dir) = (quote(unit), quote(dir));
    let mut launch = format!(
        "sh -c {} {} {q_unit} </dev/null >/dev/null 2>&1 & systemctl --user stop {q_unit} 2>/dev/null; mkdir -p {q_dir} && cd {q_dir} && rm -f in out err status attach feed nonce binding limit {BOUND_CONVERSATION} && : > in && : > out && : > err && printf %s {} > nonce && cd {} && systemd-run --user --quiet --collect --same-dir --slice=agent.slice --unit={q_unit} --property=KillMode=control-group \"--setenv=PATH=$PATH\"",
        quote(PRUNE),
        quote(&held.to_string_lossy()),
        quote(nonce),
        quote(&spec.cwd.to_string_lossy())
    );
    for property in properties {
        launch.push(' ');
        launch.push_str(&quote(&format!("--property={property}")));
    }
    for (key, value) in &spec.env {
        launch.push(' ');
        launch.push_str(&quote(&format!("--setenv={key}={}", value.to_string_lossy())));
    }
    launch.push_str(&format!(" -- sh -c {} {q_dir} {q_unit} {}", quote(HOLD), detached.as_secs()));
    for arg in &spec.argv {
        launch.push(' ');
        launch.push_str(&quote(arg));
    }
    launch
}

pub fn leave_record(record: &SessionRecord, placed: &Path) {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from).unwrap_or_default();
    if let Err(error) = mirror_through(&binary("ssh"), record, placed, &home) {
        eprintln!("north fleet: {}'s record is not on its machine, so that machine's run ledger cannot place its transcript and its broker grants it no check: {}", record.name, error.user_message());
    }
}

/// Mirrors `record` to the machine it runs on, then binds the held unit its CLI runs in there to its conversation, in that unit's own directory under `home`, so the outside broker there takes a caller in that unit for this session and no other (north#1054).
fn mirror_through(ssh: &Path, record: &SessionRecord, placed: &Path, home: &Path) -> NorthResult<()> {
    let (Some(machine), Some(conversation)) = (&record.machine, &record.conversation) else { return Ok(()) };
    let directory = placed.join(conversation);
    let file = record_file(&directory).to_string_lossy().into_owned();
    let held = held_dir(home, &record.held_unit.as_deref().map_or_else(|| unit(&record.name), str::to_owned));
    let bound = held.join(BOUND_CONVERSATION).to_string_lossy().into_owned();
    let script = format!(
        "mkdir -p {} && cat > {} && mv {} {} && {{ ! [ -d {} ] || {{ printf %s {} > {} && mv {} {}; }}; }}",
        quote(&directory.to_string_lossy()),
        quote(&format!("{file}.tmp")),
        quote(&format!("{file}.tmp")),
        quote(&file),
        quote(&held.to_string_lossy()),
        quote(conversation),
        quote(&format!("{bound}.tmp")),
        quote(&format!("{bound}.tmp")),
        quote(&bound)
    );
    let mut child = Command::new(ssh)
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", machine.session_login(), "--", &script])
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
            ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.session_login(), "--", &script]);
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
