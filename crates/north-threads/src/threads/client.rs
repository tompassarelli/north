use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::threads::{board, calibration, causes, claims, hold, incidents, ingest_lock, ingest_log, items, jobs, landing, outside, review_findings, runs, schema, snapshot, staffing, verb};
use crate::foundation::clock::Timestamp;

mod process;
mod spool;
pub use process::shell_quote;
use process::{Input, agent_event_checks, code_of, detach, execute, has_usage, python_json, spool_stem};
use spool::Spool;

const READS: [&str; 11] = [
    "job-runs", "show", "ready", "list", "holder", "unowned", "run-ids", "summary", "recommend", "calibrate",
    "routing-compare",
];
/// Each family of ledger verbs and the function that runs it; `claim` and `list` run through claims.
const PORTED: [(&[&str], verb::Verb); 10] = [
    (&jobs::VERBS, jobs::run),
    (&items::VERBS, items::run),
    (&board::VERBS, board::run),
    (&staffing::VERBS, staffing::run),
    (&runs::VERBS, runs::run),
    (&calibration::VERBS, calibration::run),
    (&landing::VERBS, landing::run),
    (&incidents::VERBS, incidents::run),
    (&causes::VERBS, causes::run),
    (&review_findings::VERBS, review_findings::run),
];
const UNREACHABLE: i32 = 255;
/// The verb a call starts detached to send the spool to the ledger host without waiting on it.
const SPOOL_FLUSH: &str = "spool-flush";
/// Set on a `queue follow` the sender has vetted against its own checkouts before forwarding it.
const FORWARDED: &str = "THREADS_FORWARDED";
const TIMED_OUT: i32 = 124;
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_WAIT: Duration = Duration::from_secs(10);
const INGEST_WAIT: Duration = Duration::from_secs(5);
const INGEST_HOLD: Duration = Duration::from_secs(12);
const SPOOL_DRAIN: Duration = Duration::from_secs(10);

fn unanswered(code: i32) -> &'static str {
    if code == TIMED_OUT { "ledger host timed out" } else { "ledger host unreachable" }
}

pub struct Client {
    home: PathBuf,
    remote: PathBuf,
    spool: PathBuf,
    snapshot: PathBuf,
    env: BTreeMap<String, String>,
    ssh: Vec<OsString>,
    fleet: Arc<dyn outside::FleetView>,
    record_wait: Duration,
    ssh_wait: Duration,
    ingest_wait: Duration,
    ingest_hold: Duration,
    spool_drain: Duration,
    /// The `north threads` a call starts without waiting on it to pull a stale snapshot or send the spool; with none, the call does both inline.
    background: Option<Vec<OsString>>,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
}

#[derive(Clone, Copy, PartialEq)]
enum Flush {
    Clear,
    Busy,
    Down,
}

pub fn run(arguments: &[String], fleet: Arc<dyn outside::FleetView>) -> u8 {
    run_in(arguments, ambient_env(), fleet)
}

pub fn run_in(arguments: &[String], env: BTreeMap<String, String>, fleet: Arc<dyn outside::FleetView>) -> u8 {
    let Some(client) = client_in(env, fleet) else {
        eprintln!("NorthThreads: no home directory");
        return 1;
    };
    match client.main(arguments, &mut io::stdin(), &mut io::stderr()) {
        Ok(code) => (code & 0xff) as u8,
        Err(error) => {
            eprintln!("NorthThreads: {error}");
            1
        }
    }
}

pub fn refresh_mirrored_snapshot() {
    let Some(client) = ambient_client(unread_fleet()) else { return };
    if let Ok(destination) = fs::read_to_string(&client.remote) {
        let _ = client.refresh_snapshot(destination.trim(), true);
    }
}

pub fn ingest(batch: &[u8]) -> Result<(), String> {
    let client = ambient_client(unread_fleet()).ok_or("no home directory")?;
    let mut err = Vec::new();
    match client.main(&["ingest".to_owned()], &mut io::Cursor::new(batch), &mut err) {
        Ok(0) => Ok(()),
        Ok(code) => Err(format!("ingest exited {code}: {}", String::from_utf8_lossy(&err).trim())),
        Err(error) => Err(error.to_string()),
    }
}

pub fn spool_ingest(batch: &[u8]) -> NorthResult<bool> {
    let Some(client) = ambient_client(unread_fleet()) else { return Ok(false) };
    if client.var("THREADS_DB").is_some() || !client.remote.exists() {
        return Ok(false);
    }
    client.write_spool(batch, "spooled by a caller that does not send at once")?;
    Ok(true)
}

/// The threads ledger this host reads: `THREADS_DB`, else the mirrored snapshot on a remote client, else the local database.
pub fn threads_database() -> PathBuf {
    if let Some(path) = std::env::var_os("THREADS_DB") {
        return path.into();
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = home.join(".local/state/threads");
    state.join(if home.join(".config/threads/remote").exists() { "snapshot.db" } else { "threads.db" })
}

pub fn ledger_path() -> NorthResult<PathBuf> {
    ambient_client(unread_fleet()).map(|client| client.ledger()).ok_or_else(|| NorthError::Usage("no home directory".into()))
}

pub fn forward_to_ledger_host(words: &[&str], input: &[u8]) -> NorthResult<Option<i32>> {
    let client = ambient_client(unread_fleet()).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    client.forward(words, input, &mut io::stderr())
}

pub fn ask_ledger_host(words: &[&str], input: &[u8]) -> NorthResult<Option<Vec<u8>>> {
    let client = ambient_client(unread_fleet()).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    client.ask(words, input)
}

fn unread_fleet() -> Arc<dyn outside::FleetView> {
    Arc::new(outside::Unread)
}

pub fn ambient_env() -> BTreeMap<String, String> {
    std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect()
}

fn ambient_client(fleet: Arc<dyn outside::FleetView>) -> Option<Client> {
    client_in(ambient_env(), fleet)
}

fn client_in(env: BTreeMap<String, String>, fleet: Arc<dyn outside::FleetView>) -> Option<Client> {
    let home = env.get("HOME").cloned()?;
    Some(Client::new(Path::new(&home), env, vec!["ssh".into()], fleet))
}

impl Client {
    fn new(home: &Path, env: BTreeMap<String, String>, ssh: Vec<OsString>, fleet: Arc<dyn outside::FleetView>) -> Self {
        Self {
            home: home.to_path_buf(),
            remote: home.join(".config/threads/remote"),
            spool: home.join(".local/state/threads/spool"),
            snapshot: home.join(".local/state/threads/snapshot.db"),
            env,
            ssh,
            fleet,
            record_wait: RECORD_WAIT,
            ssh_wait: SSH_TIMEOUT,
            ingest_wait: INGEST_WAIT,
            ingest_hold: INGEST_HOLD,
            spool_drain: SPOOL_DRAIN,
            background: Some(vec![crate::platform::north_binary::north_binary().into(), "threads".into()]),
            clock: Arc::new(Timestamp::now),
        }
    }

    fn ledger(&self) -> PathBuf {
        self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from)
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(String::as_str).filter(|value| !value.is_empty())
    }

    fn main(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        let first = argv.first().map(String::as_str);
        if first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow") {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            let vetted = if self.var(FORWARDED).is_some() { Ok(()) } else { self.vet_follows(argv, &brief) };
            if let Err(why) = vetted {
                writeln!(err, "NorthThreads: queue follow: {why}")?;
                return Ok(1);
            }
            return self.route(argv, &mut io::Cursor::new(brief.clone()), err, Some(&brief));
        }
        self.route(argv, stdin, err, None)
    }

    fn vet_follows(&self, argv: &[String], brief: &[u8]) -> Result<(), String> {
        let Some(follows) = crate::platform::follows::follows_line(&String::from_utf8_lossy(brief)) else { return Ok(()) };
        let repo = argv.iter().skip_while(|arg| *arg != "--repo").nth(1).map(String::as_str).or_else(|| argv.get(2).and_then(|item| item.split_once('#')).map(|(repo, _)| repo));
        let root = self.var("FOLLOWS_RELEASE_ROOT").map_or_else(|| self.home.join("code"), PathBuf::from);
        crate::platform::follows::vet(&follows, repo.map(|repo| root.join(repo).join("main")).as_deref())
    }

    fn route(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write, replay: Option<&[u8]>) -> NorthResult<i32> {
        let first = argv.first().map(String::as_str);
        if matches!(first, Some("snapshot-pages" | "snapshot-pull")) {
            return self.snapshot_verb(argv, stdin, err);
        }
        if first == Some(SPOOL_FLUSH) {
            return self.spool_flush_verb(argv, err);
        }
        let database = self.var("THREADS_DB").is_some();
        if first == Some("routing-compare") && !database {
            return self.local(argv, Some(&self.snapshot), replay);
        }
        if !database && self.remote.exists() {
            let dest = fs::read_to_string(&self.remote)?.trim().to_owned();
            return match self.proxy(&dest, argv, stdin, err)? {
                Some(code) => Ok(code),
                None => self.local(argv, Some(&self.snapshot), replay),
            };
        }
        if first == Some("ingest") {
            return self.ingest_locally(argv, stdin, err);
        }
        self.local(argv, None, replay)
    }

    fn ingest_locally(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        let ledger = self.ledger();
        let started = (self.clock)();
        let log = ledger.with_file_name("ingest.log");
        let record = |acquired: Option<Timestamp>, exit: i32| {
            let _ = ingest_log::append(&log, &ingest_log::Ingest { started, acquired, ended: (self.clock)(), exit, rows: ingest_log::rows(&data) });
        };
        let Some(_lock) = ingest_lock::acquire(&ledger.with_file_name("ingest.lock"), self.ingest_wait)? else {
            writeln!(err, "NorthThreads: another ingest holds the ledger; retry")?;
            record(None, ingest_lock::RETRY);
            return Ok(ingest_lock::RETRY);
        };
        let acquired = (self.clock)();
        let hold = hold::Hold::start(self.ingest_hold);
        let outcome = self.local(argv, Some(&ledger), Some(&data));
        let expired = hold.expired();
        drop(hold);
        let code = match outcome {
            Ok(code) if code == 0 || !expired => code,
            Err(error) if !expired => {
                record(Some(acquired), 1);
                return Err(error);
            }
            _ => {
                writeln!(err, "NorthThreads: ingest held the ledger past {} s; stopped it uncommitted, retry", self.ingest_hold.as_secs())?;
                ingest_lock::RETRY
            }
        };
        record(Some(acquired), code);
        Ok(code)
    }

    fn local(&self, argv: &[String], database: Option<&Path>, replay: Option<&[u8]>) -> NorthResult<i32> {
        let verb = argv.first().map_or("", String::as_str);
        let ledger = database.map(Path::to_path_buf).or_else(|| self.var("THREADS_DB").map(PathBuf::from));
        let ledger = ledger.unwrap_or_else(|| self.ledger());
        let actor = self.var("THREADS_ACTOR").or_else(|| self.var("USER")).unwrap_or("unknown").to_owned();
        let replica = ledger == self.snapshot;
        let ported = PORTED.iter().find(|(verbs, _)| verbs.contains(&verb)).map(|(_, run)| *run);
        let claim = claims::verbs::VERBS.contains(&verb);
        if ported.is_none() && !claim {
            let known: Vec<&str> = claims::verbs::VERBS.iter().chain(PORTED.iter().flat_map(|(verbs, _)| verbs.iter())).copied().chain(["snapshot-pages", "snapshot-pull", SPOOL_FLUSH]).collect();
            eprintln!("usage: north threads VERB [ARGS]\nverbs: {}", known.join(", "));
            return Ok(if matches!(verb, "-h" | "--help" | "help") { 0 } else { 2 });
        }
        if !replica && let Err(verb::Failure::Refused(why) | verb::Failure::Usage(why)) = schema::ensure(&ledger) {
            eprintln!("NorthThreads: {why}");
            return Ok(1);
        }
        let Some(run) = ported else {
            let context = claims::verbs::Context { database: &ledger, actor, no_native: self.no_native_reason(), issues: &claims::issues::GitHub, alive: (verb != claims::verbs::HOLDER).then(|| verb::Outside::alive_owners(&outside::Live::new(&self.env, self.fleet.clone()))).flatten(), now: Timestamp::now().unix_seconds(), replica };
            return Ok(claims::verbs::run(argv, &context, &mut io::stdout().lock(), &mut io::stderr()));
        };
        let live = outside::Live::new(&self.env, self.fleet.clone());
        let context = verb::Context { database: &ledger, actor, no_native: self.no_native_reason(), outside: &live, now: Timestamp::now(), replica };
        let mut stdin: Box<dyn Read> = replay.map_or_else(|| Box::new(io::stdin()) as Box<dyn Read>, |brief| Box::new(io::Cursor::new(brief.to_vec())));
        Ok(run(argv, &context, &mut stdin, &mut io::stdout().lock(), &mut io::stderr()))
    }

    fn proxy(&self, dest: &str, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<Option<i32>> {
        let first = argv.first().map(String::as_str);
        let has = |flag: &str| argv.iter().any(|arg| arg == flag);
        let mut argv = argv.to_vec();
        if first == Some("agent-event") {
            if let Err(message) = agent_event_checks(&argv) {
                writeln!(err, "NorthThreads: {message}")?;
                return Ok(Some(1));
            }
            if !argv.iter().any(|arg| arg == "--key" || arg.starts_with("--key=")) {
                let key = fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned();
                argv.extend(["--key".to_owned(), key]);
            }
        }
        if first == Some("job-run") {
            let code = self.ssh(dest, &argv, Input::Null, false, err)?;
            if !matches!(code, UNREACHABLE | TIMED_OUT) {
                return Ok(Some(code));
            }
            self.spool_argv(&argv, unanswered(code))?;
            return Ok(Some(0));
        }
        let flush = self.flush_spool_for(first, dest, err)?;
        let reachable = flush != Flush::Down;
        let clear = flush == Flush::Clear;
        if first == Some("ingest") {
            let mut data = Vec::new();
            stdin.read_to_end(&mut data)?;
            let mut state = if reachable { "busy" } else { "unreachable" };
            if clear {
                let code = self.ssh(dest, &argv, Input::Bytes(data.clone()), false, err)?;
                if code == ingest_lock::RETRY {
                    state = "busy";
                } else if code == TIMED_OUT {
                    state = "timed out";
                } else if code != UNREACHABLE {
                    if code == 0 && has_usage(&data)? {
                        self.refresh_snapshot_later(dest, true);
                    }
                    return Ok(Some(code));
                }
            }
            let path = self.write_spool(&data, &format!("ledger host {state}"))?;
            writeln!(err, "NorthThreads: {dest} {state}; spooled ingest to {}", path.display())?;
            return Ok(Some(0));
        }
        if first == Some("agent-event") {
            let code = if clear { self.ssh(dest, &argv, Input::Null, false, err)? } else { UNREACHABLE };
            if !matches!(code, UNREACHABLE | TIMED_OUT) {
                return Ok(Some(code));
            }
            self.spool_argv(&argv, unanswered(code))?;
            return Ok(Some(0));
        }
        let follow = first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow");
        let input = if follow {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            Input::Bytes(brief)
        } else {
            Input::Inherit
        };
        let code = if reachable { self.ssh(dest, &argv, input, false, err)? } else { UNREACHABLE };
        if first == Some("agent-events") || first == Some("agent-event-state") {
            if code == UNREACHABLE {
                writeln!(err, "NorthThreads: {dest} unreachable; canonical events unavailable")?;
            }
            return Ok(Some(code));
        }
        if code == TIMED_OUT {
            return Ok(Some(code));
        }
        if code != UNREACHABLE {
            let calibrate_write = first == Some("calibrate") && has("--write");
            if !(code == 0 && calibrate_write && has("--goal-row")) {
                if code == 0 && (first == Some("record-priors") || calibrate_write) {
                    self.refresh_snapshot(dest, true)?;
                } else {
                    self.refresh_snapshot_later(dest, false);
                }
            }
            return Ok(Some(code));
        }
        if first.is_some_and(|verb| READS.contains(&verb)) && self.snapshot.exists() {
            writeln!(err, "{}", self.stale_line(dest)?)?;
            return Ok(None);
        }
        writeln!(err, "NorthThreads: {dest} unreachable; the ledger lives there, so nothing was read or recorded")?;
        Ok(Some(code))
    }

    fn stale_line(&self, dest: &str) -> NorthResult<String> {
        let modified = fs::metadata(&self.snapshot)?.modified()?;
        Ok(format!("STALE: ledger snapshot from {}; {dest} unreachable", Timestamp::from(modified).iso()))
    }

    fn spool_records(&self) -> NorthResult<Spool> {
        Spool::open(&self.spool, &self.home.join(".local/state/north/ledger.db"), (self.clock)())
    }

    fn queue_spooled(&self, tmp: &Path, path: &Path, cause: &str) -> NorthResult<()> {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.spool_records()?.queue(&name, cause, (self.clock)())?;
        Ok(fs::rename(tmp, path)?)
    }

    fn write_spool(&self, data: &[u8], cause: &str) -> NorthResult<PathBuf> {
        fs::create_dir_all(&self.spool)?;
        let path = self.spool.join(format!("{}.jsonl", spool_stem()));
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, data)?;
        self.queue_spooled(&tmp, &path, cause)?;
        Ok(path)
    }

    fn spool_argv(&self, argv: &[String], cause: &str) -> NorthResult<()> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.spool)?;
        fs::set_permissions(&self.spool, fs::Permissions::from_mode(0o700))?;
        let path = self.spool.join(format!("{}.argv", spool_stem()));
        let tmp = path.with_extension("tmp");
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?.write_all(python_json(argv).as_bytes())?;
        self.queue_spooled(&tmp, &path, cause)
    }

    /// The spool's lock, or none while another call flushes it: that call sends every spooled write in order, so this one neither waits behind it nor sends any itself.
    fn spool_lock(&self) -> NorthResult<Option<File>> {
        let lock = File::create(self.spool.join(".lock"))?;
        match lock.try_lock() {
            Ok(()) => Ok(Some(lock)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
        }
    }

    fn flush_spool(&self, dest: &str, err: &mut dyn Write, budget: Option<Duration>) -> NorthResult<Flush> {
        if !self.spool.is_dir() {
            return Ok(Flush::Clear);
        }
        let Some(_lock) = self.spool_lock()? else { return Ok(Flush::Busy) };
        let mut spool = self.spool_records()?;
        let started = Instant::now();
        for name in spool.queued()? {
            if budget.is_some_and(|budget| started.elapsed() >= budget) {
                return Ok(Flush::Busy);
            }
            let path = self.spool.join(&name);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if !spool.take(&name, (self.clock)())? {
                return Ok(Flush::Busy);
            }
            let code = match self.send_spooled(&path, file, dest, err) {
                Ok(code) => code,
                Err(error) => {
                    spool.release(&name, &format!("the flush failed before the ledger host answered: {error}"), (self.clock)())?;
                    return Err(error);
                }
            };
            if let Some(stop) = self.settle_spooled(&mut spool, &name, &path, code, err)? {
                return Ok(stop);
            }
        }
        Ok(Flush::Clear)
    }

    fn settle_spooled(&self, spool: &mut Spool, name: &str, path: &Path, code: i32, err: &mut dyn Write) -> NorthResult<Option<Flush>> {
        if code == UNREACHABLE {
            spool.release(name, "ledger host unreachable", (self.clock)())?;
            return Ok(Some(Flush::Down));
        }
        if matches!(code, ingest_lock::RETRY | TIMED_OUT) {
            spool.release(name, if code == TIMED_OUT { "ledger host timed out" } else { "ledger host busy" }, (self.clock)())?;
            return Ok(Some(Flush::Busy));
        }
        if code != 0 {
            spool.refuse(name, &format!("the ledger host exited {code}"), (self.clock)())?;
            writeln!(err, "NorthThreads: remote ingest refused {}; it will not be sent again", path.display())?;
            return Ok(None);
        }
        spool.deliver(name, (self.clock)())?;
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(None),
        }
    }

    fn send_spooled(&self, path: &Path, mut file: File, dest: &str, err: &mut dyn Write) -> NorthResult<i32> {
        if path.extension().is_some_and(|extension| extension == "argv") {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let args: Vec<String> = serde_json::from_str(&text)?;
            let quiet = args.first().is_some_and(|verb| verb == "agent-event");
            self.ssh(dest, &args, Input::Null, quiet, err)
        } else {
            self.ssh(dest, &["ingest".to_owned()], Input::File(file), false, err)
        }
    }

    fn refresh_snapshot(&self, dest: &str, force: bool) -> NorthResult<()> {
        let _ = snapshot::refresh(&self.pull_argv(dest), &self.snapshot, force, snapshot::PULL_TIMEOUT);
        Ok(())
    }

    fn refresh_snapshot_later(&self, dest: &str, force: bool) {
        let Some(program) = &self.background else {
            let _ = self.refresh_snapshot(dest, force);
            return;
        };
        if !snapshot::take_turn(&self.snapshot, force).unwrap_or(false) {
            return;
        }
        detach(program, &["snapshot-pull".as_ref(), "--force".as_ref(), "--snapshot".as_ref(), self.snapshot.as_os_str(), dest.as_ref()]);
    }

    /// A spooled write sends the spool before itself, because its order behind the spool matters; any other call leaves the spool to a detached `spool-flush`, because one spooled usage batch holds the ledger host for seconds and a bounded call such as a launch's `holder` must not wait on them.
    fn flush_spool_for(&self, verb: Option<&str>, dest: &str, err: &mut dyn Write) -> NorthResult<Flush> {
        let Some(program) = self.background.as_ref().filter(|_| !matches!(verb, Some("ingest" | "agent-event"))) else {
            let flush = self.flush_spool(dest, err, Some(self.spool_drain))?;
            if let (Flush::Busy, Some(program)) = (flush, self.background.as_ref()) {
                detach(program, &[SPOOL_FLUSH.as_ref(), dest.as_ref()]);
            }
            return Ok(flush);
        };
        if self.spool_pending()? {
            detach(program, &[SPOOL_FLUSH.as_ref(), dest.as_ref()]);
        }
        Ok(Flush::Clear)
    }

    fn spool_pending(&self) -> NorthResult<bool> {
        if !self.spool.is_dir() {
            return Ok(false);
        }
        Ok(!self.spool_records()?.queued()?.is_empty())
    }

    fn spool_flush_verb(&self, argv: &[String], err: &mut dyn Write) -> NorthResult<i32> {
        let [_, dest] = argv else {
            return Err(NorthError::Usage(format!("usage: north threads {SPOOL_FLUSH} DEST")));
        };
        Ok(match self.flush_spool(dest, err, None)? {
            Flush::Clear => 0,
            Flush::Busy => ingest_lock::RETRY,
            Flush::Down => UNREACHABLE,
        })
    }

    fn pull_argv(&self, dest: &str) -> Vec<OsString> {
        self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", "-o", crate::platform::ssh::OWN_CONNECTION, dest, snapshot::REMOTE_COMMAND])
    }

    fn snapshot_verb(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        if argv[0] == "snapshot-pages" {
            let database = self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from);
            return match snapshot::serve(&database, self.no_native_reason().is_some(), stdin, &mut io::stdout().lock()) {
                Ok(()) => Ok(0),
                Err(error) => {
                    writeln!(err, "NorthThreads: snapshot pages failed: {error}")?;
                    Ok(1)
                }
            };
        }
        let (mut force, mut path, mut dest) = (false, self.snapshot.clone(), None);
        let mut rest = argv[1..].iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--force" => force = true,
                "--snapshot" => path = rest.next().map(PathBuf::from).ok_or_else(|| NorthError::Usage("--snapshot needs a path".into()))?,
                _ if dest.is_none() && !arg.starts_with('-') => dest = Some(arg.clone()),
                _ => return Err(NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into())),
            }
        }
        let dest = dest.ok_or_else(|| NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into()))?;
        match snapshot::refresh(&self.pull_argv(&dest), &path, force, snapshot::PULL_TIMEOUT) {
            Ok(_) => Ok(0),
            Err(error) => {
                writeln!(err, "NorthThreads: snapshot pull from {dest} failed: {error}")?;
                Ok(1)
            }
        }
    }

    fn no_native_reason(&self) -> Option<&'static str> {
        if self.var("THREADS_NO_NATIVE").is_some() {
            return Some("THREADS_NO_NATIVE is set (agy)");
        }
        self.var("CLAUDE_CODE_REMOTE").is_some().then_some("CLAUDE_CODE_REMOTE is set (cloud worker)")
    }

    fn ssh(&self, dest: &str, args: &[String], input: Input, quiet: bool, err: &mut dyn Write) -> NorthResult<i32> {
        let mut remote = Vec::new();
        if let Some(actor) = self.var("THREADS_ACTOR") {
            remote.push(format!("THREADS_ACTOR={actor}"));
        }
        if self.no_native_reason().is_some() {
            remote.push("THREADS_NO_NATIVE=1".to_owned());
        }
        let first = args.first().map(String::as_str);
        if matches!(first, Some("unowned" | "claim" | "list")) && let Some(owners) = self.fleet.live_owners() {
            remote.push(format!("THREADS_ALIVE_OWNERS={}", python_json(&owners.into_iter().collect::<Vec<_>>())));
        }
        if first == Some("queue") && args.get(1).map(String::as_str) == Some("follow") {
            remote.push(format!("{FORWARDED}=1"));
        }
        if !remote.is_empty() {
            remote.insert(0, "env".to_owned());
        }
        remote.extend(["north".to_owned(), "threads".to_owned()]);
        remote.extend(args.iter().cloned());
        let command = remote.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, &command]);
        let stdout = if quiet { Stdio::null() } else { Stdio::inherit() };
        let wait = if first == Some("job-run") { self.record_wait } else { self.ssh_wait };
        match execute(&argv, input, stdout, wait)? {
            Some(status) => Ok(code_of(status)),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok(TIMED_OUT)
            }
        }
    }

    fn forward(&self, words: &[&str], input: &[u8], err: &mut dyn Write) -> NorthResult<Option<i32>> {
        if self.var("THREADS_DB").is_some() || !self.remote.exists() {
            return Ok(None);
        }
        let dest = fs::read_to_string(&self.remote)?.trim().to_owned();
        let command = words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", &dest, &command]);
        match execute(&argv, Input::Bytes(input.to_vec()), Stdio::inherit(), SSH_TIMEOUT)? {
            Some(status) => Ok(Some(code_of(status))),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok(Some(TIMED_OUT))
            }
        }
    }

    fn ask(&self, words: &[&str], input: &[u8]) -> NorthResult<Option<Vec<u8>>> {
        if self.var("THREADS_DB").is_some() || !self.remote.exists() {
            return Ok(None);
        }
        let dest = fs::read_to_string(&self.remote)?.trim().to_owned();
        let command = words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", &dest, &command]);
        let mut output = tempfile::tempfile()?;
        match execute(&argv, Input::Bytes(input.to_vec()), Stdio::from(output.try_clone()?), SSH_TIMEOUT)? {
            Some(status) if status.success() => {
                let mut answer = Vec::new();
                output.seek(io::SeekFrom::Start(0))?;
                output.read_to_end(&mut answer)?;
                Ok(Some(answer))
            }
            Some(status) => Err(NorthError::Refused(format!("{dest} answered exit {}", code_of(status)))),
            None => Err(NorthError::Refused(format!("{dest} timed out; remote outcome unknown"))),
        }
    }

    fn ssh_argv(&self, arguments: &[&str]) -> Vec<OsString> {
        crate::platform::ssh::argv(&self.ssh, &self.env, arguments)
    }
}


#[cfg(test)]
mod tests;
