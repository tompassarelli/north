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
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use north_session_host::held::{HOLD, PRUNE, unit};

use crate::error::{NorthError, NorthResult};
use crate::platform::process::try_run_command_within;
use crate::platform::session_host_files::{BOUND_CONVERSATION, HELD};

use super::link::{Link, channel, quote, standing_master};
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
        Some(machine) => over_ssh(spec, record.conversation.as_deref(), machine, &record.name, trace),
        None => Ok(spec),
    }
}

/// Whether `account`'s profile would start a session without hooks where it runs, read beside the launch's other work: over ssh to a build host it costs two round trips, which the session host's start and the registration overlap, and the launch takes its verdict before it records the host.
pub struct HooksCheck(JoinHandle<Option<String>>);

impl HooksCheck {
    pub fn start(runtime: Arc<dyn Runtime>, account: &Account, machine: Option<&Machine>) -> Self {
        let (account, machine) = (account.clone(), machine.cloned());
        let reads = ProfileReads::shared();
        Self(std::thread::spawn(move || {
            let _reads = reads.map(LaunchReads::enter);
            unguarded_where_it_runs(runtime.as_ref(), &account, machine.as_ref())
        }))
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
    answer_on(machine, &format!("[ -e {quoted} ] || exit {ABSENT}; cat -- {quoted}"))
}

/// Held by a launch, so each answer it asks a build host for is asked there once: a launch reads its profile and its agent runtime link for the spare it may take, again for the host it starts and its profile once more for its hooks verdict, and each ask is an ssh round trip of about half a second. The answers belong to that launch alone, on the thread that holds it and on the threads it hands [`ProfileReads::shared`] to; another launch asks afresh.
pub struct ProfileReads {
    _launch: LaunchReads,
    before: Option<LaunchReads>,
}

impl ProfileReads {
    pub fn hold() -> Self {
        LaunchReads(Arc::new(Launch(LAUNCHES.fetch_add(1, Ordering::Relaxed)))).enter()
    }

    /// The reads this thread's launch holds, for a thread it starts to enter.
    pub fn shared() -> Option<LaunchReads> {
        let held = LAUNCH_READS.take();
        LAUNCH_READS.set(held.clone());
        held
    }
}

impl Drop for ProfileReads {
    fn drop(&mut self) {
        LAUNCH_READS.set(self.before.take());
    }
}

/// One launch's answers, shared by the threads that launch starts; they are forgotten when its last handle drops.
#[derive(Clone)]
pub struct LaunchReads(Arc<Launch>);

impl LaunchReads {
    /// Makes this thread read through these answers until the returned guard drops.
    pub fn enter(self) -> ProfileReads {
        ProfileReads { before: LAUNCH_READS.replace(Some(self.clone())), _launch: self }
    }

    fn seen<T>(&self, answer: impl FnOnce(&mut Answers) -> T) -> T {
        answer(profile_reads().entry(self.0.0).or_default())
    }
}

struct Launch(u64);

impl Drop for Launch {
    fn drop(&mut self) {
        profile_reads().remove(&self.0);
    }
}

type Answers = BTreeMap<(String, String), Seen>;

static PROFILE_READS: Mutex<BTreeMap<u64, Answers>> = Mutex::new(BTreeMap::new());
static LAUNCHES: AtomicU64 = AtomicU64::new(0);

fn profile_reads() -> MutexGuard<'static, BTreeMap<u64, Answers>> {
    PROFILE_READS.lock().unwrap_or_else(PoisonError::into_inner)
}

thread_local! {
    static LAUNCH_READS: Cell<Option<LaunchReads>> = const { Cell::new(None) };
}

/// What `script` prints on `machine` as its session login, sent as a channel of the standing link master to that login, which the session's own link rides next: each launch read of a build host before its session host starts was a fresh handshake. On a thread that holds a launch's [`ProfileReads`], an answer that launch already read there is reused.
pub fn answer_on(machine: &Machine, script: &str) -> Seen {
    let key = (machine.session_login().to_owned(), script.to_owned());
    let launch = ProfileReads::shared();
    if let Some(seen) = launch.as_ref().and_then(|reads| reads.seen(|seen| seen.get(&key).cloned())) {
        return seen;
    }
    let ssh = binary("ssh");
    let seen = answer_through(&ssh, standing_master(&ssh, machine.session_login()).as_deref(), machine, script);
    if let Some(reads) = launch.filter(|_| !matches!(seen, Seen::Unreachable)) {
        reads.seen(|answers| answers.insert(key, seen.clone()));
    }
    seen
}

pub(super) fn answer_through(ssh: &Path, master: Option<&Path>, machine: &Machine, script: &str) -> Seen {
    let mut ssh = Command::new(ssh);
    ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]).args(channel(master)).args([machine.session_login(), "--", script]);
    match try_run_command_within(ssh, None, SSH_WAIT).ok().flatten() {
        Some(output) if output.status.success() => Seen::Text(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(output) if output.status.code() == Some(ABSENT) => Seen::Absent,
        _ => Seen::Unreachable,
    }
}

pub fn over_ssh(spec: HostSpec, conversation: Option<&str>, machine: &Machine, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or_else(|| NorthError::State("HOME is not set".into()))?;
    held_under(spec, conversation, machine, Path::new(&home), session, trace)
}

/// The spec `machine` starts a session from: its working directory moved under the session login's own home, so an agent login stages in the checkout it reads and never in the operator's.
pub fn staged_on(spec: HostSpec, machine: &Machine, home: &Path) -> HostSpec {
    HostSpec { cwd: machine.session_path(&spec.cwd, home), ..spec }
}

pub(super) fn held_under(spec: HostSpec, conversation: Option<&str>, machine: &Machine, home: &Path, session: &str, trace: &Path) -> NorthResult<HostSpec> {
    let spec = staged_on(spec, machine, home);
    let held = machine.session_home(home).unwrap_or_else(|| home.to_path_buf()).join(HELD);
    let unit = unit(&spec.key);
    let dir = held.join(&unit).to_string_lossy().into_owned();
    let nonce = new_uuid()?;
    let launch = launch_script(&spec, &held, &Starting { unit: &unit, dir: &dir, nonce: &nonce, conversation }, DETACHED, &[]);
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

/// The held unit a launch starts: its name, its directory, the nonce a link attaches with and the conversation it is bound to, or None to leave it bound to none.
pub(super) struct Starting<'a> {
    pub unit: &'a str,
    pub dir: &'a str,
    pub nonce: &'a str,
    pub conversation: Option<&'a str>,
}

/// The script that starts `spec`'s CLI as the held unit `starting` under `held`, stopped once no link has been attached for `detached`, with `properties` added to the unit.
pub(super) fn launch_script(spec: &HostSpec, held: &Path, starting: &Starting, detached: Duration, properties: &[String]) -> String {
    let Starting { unit, dir, nonce, conversation } = *starting;
    let (q_unit, q_dir) = (quote(unit), quote(dir));
    let binding = conversation.map_or_else(|| format!(" && rm -f {BOUND_CONVERSATION}"), |conversation| format!(" && printf %s {} > {BOUND_CONVERSATION}.launch && mv {BOUND_CONVERSATION}.launch {BOUND_CONVERSATION}", quote(conversation)));
    let mut launch = format!(
        "sh -c {} {} {q_unit} </dev/null >/dev/null 2>&1 & systemctl --user stop {q_unit} 2>/dev/null; mkdir -p {q_dir} && cd {q_dir} && rm -f in out err status attach feed nonce binding limit && : > in && : > out && : > err && printf %s {} > nonce{binding} && cd {} && systemd-run --user --quiet --collect --same-dir --slice=agent.slice --unit={q_unit} --property=KillMode=control-group --property=IOAccounting=yes \"--setenv=PATH=$PATH\"",
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
pub(super) fn mirror_through(ssh: &Path, record: &SessionRecord, placed: &Path, home: &Path) -> NorthResult<()> {
    let (Some(machine), Some(conversation)) = (&record.machine, &record.conversation) else { return Ok(()) };
    let record = &SessionRecord { cwd: machine.session_path(&record.cwd, home), ..record.clone() };
    let directory = machine.session_path(&placed.join(conversation), home);
    let file = record_file(&directory).to_string_lossy().into_owned();
    let held = held_dir(&machine.session_path(home, home), &record.held_unit.as_deref().map_or_else(|| unit(&record.name), str::to_owned));
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
