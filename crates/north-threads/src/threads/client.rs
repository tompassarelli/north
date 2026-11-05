use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::threads::{board, calibration, claims, hold, incidents, ingest_lock, ingest_log, items, jobs, landing, outside, review_findings, runs, schema, snapshot, staffing, verb};
use crate::foundation::clock::Timestamp;

const READS: [&str; 11] = [
    "job-runs", "show", "ready", "list", "holder", "unowned", "run-ids", "summary", "recommend", "calibrate",
    "routing-compare",
];
/// Each family of ledger verbs and the function that runs it; `claim` and `list` run through claims.
const PORTED: [(&[&str], verb::Verb); 9] = [
    (&jobs::VERBS, jobs::run),
    (&items::VERBS, items::run),
    (&board::VERBS, board::run),
    (&staffing::VERBS, staffing::run),
    (&runs::VERBS, runs::run),
    (&calibration::VERBS, calibration::run),
    (&landing::VERBS, landing::run),
    (&incidents::VERBS, incidents::run),
    (&review_findings::VERBS, review_findings::run),
];
const UNREACHABLE: i32 = 255;
/// Set on a `queue follow` the sender has vetted against its own checkouts before forwarding it.
const FORWARDED: &str = "THREADS_FORWARDED";
const TIMED_OUT: i32 = 124;
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_WAIT: Duration = Duration::from_secs(10);
const INGEST_WAIT: Duration = Duration::from_secs(5);
const INGEST_HOLD: Duration = Duration::from_secs(12);

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
    /// The `snapshot-pull` a call that found the snapshot stale starts without waiting on it; with none, the call pulls inline.
    background_pull: Option<Vec<OsString>>,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
}

#[derive(PartialEq)]
enum Flush {
    Clear,
    Busy,
    Down,
}

enum Input {
    Inherit,
    Null,
    Bytes(Vec<u8>),
    File(File),
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
    client.write_spool(batch)?;
    Ok(true)
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
            background_pull: Some(vec![crate::platform::north_binary::north_binary().into(), "threads".into(), "snapshot-pull".into()]),
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
        if first == Some("snapshot-pages") || first == Some("snapshot-pull") {
            return self.snapshot_verb(argv, stdin, err);
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
            let _ = ingest_log::append(&log, &ingest_log::Ingest { started, acquired, ended: (self.clock)(), exit });
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
            let known: Vec<&str> = claims::verbs::VERBS.iter().chain(PORTED.iter().flat_map(|(verbs, _)| verbs.iter())).copied().chain(["snapshot-pages", "snapshot-pull"]).collect();
            eprintln!("usage: north threads VERB [ARGS]\nverbs: {}", known.join(", "));
            return Ok(if matches!(verb, "-h" | "--help" | "help") { 0 } else { 2 });
        }
        if !replica && let Err(verb::Failure::Refused(why) | verb::Failure::Usage(why)) = schema::ensure(&ledger) {
            eprintln!("NorthThreads: {why}");
            return Ok(1);
        }
        let Some(run) = ported else {
            let context = claims::verbs::Context { database: &ledger, actor, no_native: self.no_native_reason(), issues: &claims::issues::GitHub, alive: (verb != claims::verbs::HOLDER).then(|| verb::Outside::alive_owners(&outside::Live::new(&self.env, self.fleet.clone()))).flatten(), now: Timestamp::now().unix_seconds() };
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
            if code != UNREACHABLE && code != TIMED_OUT {
                return Ok(Some(code));
            }
            self.spool_argv(&argv)?;
            return Ok(Some(0));
        }
        let flush = self.flush_spool(dest, err)?;
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
                        self.refresh_snapshot(dest, true)?;
                    }
                    return Ok(Some(code));
                }
            }
            let path = self.write_spool(&data)?;
            writeln!(err, "NorthThreads: {dest} {state}; spooled ingest to {}", path.display())?;
            return Ok(Some(0));
        }
        if first == Some("agent-event") {
            let code = if clear { self.ssh(dest, &argv, Input::Null, false, err)? } else { UNREACHABLE };
            if code != UNREACHABLE && code != TIMED_OUT {
                return Ok(Some(code));
            }
            self.spool_argv(&argv)?;
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
                    self.refresh_snapshot_later(dest);
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

    fn write_spool(&self, data: &[u8]) -> NorthResult<PathBuf> {
        fs::create_dir_all(&self.spool)?;
        let path = self.spool.join(format!("{}.jsonl", spool_stem()));
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, data)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    fn spool_argv(&self, argv: &[String]) -> NorthResult<()> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.spool)?;
        fs::set_permissions(&self.spool, fs::Permissions::from_mode(0o700))?;
        let path = self.spool.join(format!("{}.argv", spool_stem()));
        let tmp = path.with_extension("tmp");
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?.write_all(python_json(argv).as_bytes())?;
        Ok(fs::rename(&tmp, &path)?)
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

    fn flush_spool(&self, dest: &str, err: &mut dyn Write) -> NorthResult<Flush> {
        if !self.spool.is_dir() {
            return Ok(Flush::Clear);
        }
        let Some(_lock) = self.spool_lock()? else { return Ok(Flush::Busy) };
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.spool)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl" || extension == "argv"))
            .collect();
        paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        for path in paths {
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let code = if path.extension().is_some_and(|extension| extension == "argv") {
                let mut text = String::new();
                file.read_to_string(&mut text)?;
                let args: Vec<String> = serde_json::from_str(&text)?;
                let quiet = args.first().is_some_and(|verb| verb == "agent-event");
                self.ssh(dest, &args, Input::Null, quiet, err)?
            } else {
                self.ssh(dest, &["ingest".to_owned()], Input::File(file), false, err)?
            };
            if code == UNREACHABLE {
                return Ok(Flush::Down);
            }
            if code == ingest_lock::RETRY || code == TIMED_OUT {
                return Ok(Flush::Busy);
            }
            if code != 0 {
                fs::rename(&path, path.with_extension("failed"))?;
                writeln!(err, "NorthThreads: remote ingest refused {}; kept as .failed", path.display())?;
                continue;
            }
            match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(Flush::Clear)
    }

    fn refresh_snapshot(&self, dest: &str, force: bool) -> NorthResult<()> {
        let _ = snapshot::refresh(&self.pull_argv(dest), &self.snapshot, force, snapshot::PULL_TIMEOUT);
        Ok(())
    }

    fn refresh_snapshot_later(&self, dest: &str) {
        let Some(program) = &self.background_pull else {
            let _ = self.refresh_snapshot(dest, false);
            return;
        };
        if !snapshot::take_turn(&self.snapshot, false).unwrap_or(false) {
            return;
        }
        let mut command = Command::new(&program[0]);
        command.args(&program[1..]).arg("--force").arg("--snapshot").arg(&self.snapshot).arg(dest);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
        if let Ok(mut child) = command.spawn() {
            std::thread::spawn(move || child.wait());
        }
    }

    fn pull_argv(&self, dest: &str) -> Vec<OsString> {
        self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, snapshot::REMOTE_COMMAND])
    }

    fn snapshot_verb(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        if argv[0] == "snapshot-pages" {
            if let Some(reason) = self.no_native_reason() {
                writeln!(err, "NorthThreads: the snapshot holds native items: {reason}")?;
                return Ok(1);
            }
            let database = self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from);
            return match snapshot::serve(&database, stdin, &mut io::stdout().lock()) {
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

fn agent_event_checks(argv: &[String]) -> Result<(), String> {
    let option = |name: &str| {
        let mut found = None;
        let prefix = format!("{name}=");
        for (index, arg) in argv.iter().enumerate() {
            if arg == name {
                found = argv.get(index + 1).cloned();
            } else if let Some(value) = arg.strip_prefix(&prefix) {
                found = Some(value.to_owned());
            }
        }
        found
    };
    if let Some(body) = option("--body") {
        outside::betterleaks_clean(&body)?;
    }
    if option("--key").is_some_and(|key| key.trim().is_empty()) {
        return Err("key must be nonempty text".into());
    }
    Ok(())
}

fn feed(mut child: Child, bytes: Option<Vec<u8>>) -> Child {
    if let (Some(bytes), Some(mut stdin)) = (bytes, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    child
}

fn execute(argv: &[OsString], input: Input, stdout: Stdio, wait: Duration) -> NorthResult<Option<ExitStatus>> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdout(stdout);
    let bytes = match input {
        Input::Inherit => None,
        Input::Null => {
            command.stdin(Stdio::null());
            None
        }
        Input::File(file) => {
            command.stdin(file);
            None
        }
        Input::Bytes(bytes) => {
            command.stdin(Stdio::piped());
            Some(bytes)
        }
    };
    let mut child = feed(command.spawn()?, bytes);
    let deadline = Instant::now() + wait;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Some(status))
}

fn code_of(status: ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|signal| -signal)).unwrap_or(1)
}

fn has_usage(data: &[u8]) -> NorthResult<bool> {
    for line in data.split(|byte| *byte == b'\n') {
        if line.trim_ascii().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(line)?;
        if row.get("type").and_then(Value::as_str) == Some("usage") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn spool_stem() -> String {
    format!("{}-{}", Timestamp::now().unix_nanos(), std::process::id())
}

pub fn shell_quote(word: &str) -> String {
    if word.is_empty() {
        return "''".into();
    }
    if word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte)) {
        return word.into();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

fn python_json(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| {
        let mut text = String::from("\"");
        for unit in item.encode_utf16() {
            match unit {
                0x22 => text.push_str("\\\""),
                0x5c => text.push_str("\\\\"),
                0x0a => text.push_str("\\n"),
                0x0d => text.push_str("\\r"),
                0x09 => text.push_str("\\t"),
                0x08 => text.push_str("\\b"),
                0x0c => text.push_str("\\f"),
                0x20..=0x7e => text.push(unit as u8 as char),
                _ => text.push_str(&format!("\\u{unit:04x}")),
            }
        }
        text.push('"');
        text
    }).collect();
    format!("[{}]", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::collections::BTreeSet;

    /// The live owners a test writes to a JSON file; it opens no gate.
    struct FleetFile(PathBuf);

    impl outside::FleetView for FleetFile {
        fn live_owners(&self) -> Option<BTreeSet<String>> {
            serde_json::from_slice::<Vec<String>>(&fs::read(&self.0).ok()?).ok().map(|owners| owners.into_iter().collect())
        }

        fn gated_issues(&self) -> BTreeMap<String, String> {
            BTreeMap::new()
        }
    }

    struct Rig {
        _directory: tempfile::TempDir,
        root: PathBuf,
        client: Client,
    }

    const FAKE_SSH: &str = r#"for a; do last=$a; done
printf '%s\n' "$*" >> "$ROOT/ssh.log"
[ -f "$ROOT/sleep" ] && exec sleep "$(cat "$ROOT/sleep")"
case "$last" in
  *ingest) cat >> "$ROOT/ingest.log"; [ -f "$ROOT/ingest-code" ] && exit "$(cat "$ROOT/ingest-code")" ;;
  "north threads snapshot-pages") cat >/dev/null; cat "$ROOT/pages" ;;
  "north fleet row put") { cat; echo; } >> "$ROOT/rows.log" ;;
esac
exit "$(cat "$ROOT/code")"
"#;

    fn rig(env: &[(&str, &str)]) -> Rig {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        fs::create_dir_all(root.join(".config/threads")).unwrap();
        fs::create_dir_all(root.join(".local/state/threads")).unwrap();
        fs::write(root.join(".config/threads/remote"), "ledger-host\n").unwrap();
        fs::write(root.join("code"), "0").unwrap();
        fs::write(root.join("fleet.json"), "[]\n").unwrap();
        let ledger = root.join("ledger.db");
        rusqlite::Connection::open(&ledger).unwrap().execute_batch("CREATE TABLE t(v); INSERT INTO t VALUES (1);").unwrap();
        let mut pages = Vec::new();
        snapshot::serve(&ledger, &mut &snapshot::manifest_bytes(0, &[])[..], &mut pages).unwrap();
        fs::write(root.join("pages"), pages).unwrap();
        let ssh = format!("ROOT='{}'\n{FAKE_SSH}", root.display());
        let mut variables: BTreeMap<String, String> = env.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        variables.insert("HOME".into(), root.display().to_string());
        let fleet_file = root.join("fleet.json");
        let fleet: Arc<dyn outside::FleetView> = Arc::new(FleetFile(fleet_file));
        let mut client = Client::new(
            &root,
            variables,
            vec!["/bin/sh".into(), "-c".into(), ssh.into(), "ssh".into()],
            fleet,
        );
        client.background_pull = None;
        Rig { _directory: directory, root, client }
    }

    fn call(rig: &Rig, argv: &[&str], stdin: &[u8]) -> (i32, String) {
        let argv: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
        let mut err = Vec::new();
        let code = rig.client.main(&argv, &mut &stdin[..], &mut err).unwrap();
        (code, String::from_utf8(err).unwrap())
    }

    fn log(rig: &Rig, name: &str) -> Vec<String> {
        fs::read_to_string(rig.root.join(name)).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    const PREFIX: &str = "-o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET ledger-host";

    #[test]
    fn threads_client_spools_unreachable_writes_and_flushes_them_in_order() {
        let rig = rig(&[]);
        fs::write(rig.root.join("code"), "255").unwrap();
        assert_eq!(call(&rig, &["job-run", "nightly", "--host", "laptop"], b"").0, 0);
        let (code, err) = call(&rig, &["ingest"], b"{\"type\": \"run\"}\n");
        assert_eq!(code, 0);
        assert!(err.contains("ledger-host unreachable; spooled ingest to "), "{err}");
        assert_eq!(fs::metadata(&rig.client.spool).unwrap().mode() & 0o777, 0o700);
        let packets: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "argv")).collect();
        assert_eq!(packets.len(), 1);
        assert_eq!(fs::metadata(&packets[0]).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::read_to_string(&packets[0]).unwrap(), r#"["job-run", "nightly", "--host", "laptop"]"#);
        let attempts = log(&rig, "ssh.log").len();
        fs::write(rig.root.join("code"), "0").unwrap();
        assert_eq!(call(&rig, &["list"], b"").0, 0);
        assert_eq!(log(&rig, "ssh.log")[attempts..], [
            format!("{PREFIX} north threads job-run nightly --host laptop"),
            format!("{PREFIX} north threads ingest"),
            format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads list"),
            format!("{PREFIX} north threads snapshot-pages"),
        ]);
        assert_eq!(log(&rig, "ingest.log"), ["{\"type\": \"run\"}"]);
        let left: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, [".lock"]);
    }

    #[test]
    fn a_stalled_ledger_host_returns_a_claim_as_outcome_unknown_within_its_bound_and_never_replays_it() {
        let mut rig = rig(&[]);
        rig.client.ssh_wait = Duration::from_millis(300);
        fs::write(rig.root.join("sleep"), "30").unwrap();
        let begun = Instant::now();
        assert_eq!(call(&rig, &["claim", "wisp#99", "--by", "stall"], b""), (TIMED_OUT, "NorthThreads: ledger-host timed out; remote outcome unknown\n".to_owned()));
        assert!(begun.elapsed() < Duration::from_secs(5), "{:?}", begun.elapsed());
        assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'wisp#99' --by stall")]);
        assert!(!rig.client.spool.exists(), "a timed-out claim was queued for replay");
    }

    fn spooled(rig: &Rig) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&rig.client.spool).map(|entries| entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| name != ".lock").collect()).unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_spooled_batch_whose_flush_times_out_stays_spooled_and_is_sent_again_once_the_host_answers() {
        let mut rig = rig(&[]);
        rig.client.ssh_wait = Duration::from_millis(300);
        let kept = rig.client.write_spool(b"{\"n\": 1}\n").unwrap();
        fs::write(rig.root.join("sleep"), "30").unwrap();
        let (_, err) = call(&rig, &["claim", "north#63"], b"");
        assert!(!err.contains("refused"), "{err}");
        assert_eq!(spooled(&rig), [kept.file_name().unwrap().to_string_lossy().into_owned()], "a timed-out flush is neither .failed nor dropped");
        fs::remove_file(rig.root.join("sleep")).unwrap();
        assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
        assert!(spooled(&rig).is_empty());
        assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);
    }

    fn stalled_rig() -> Rig {
        let mut rig = rig(&[]);
        rig.client.ssh_wait = Duration::from_millis(300);
        fs::write(rig.root.join("sleep"), "30").unwrap();
        rig
    }

    #[test]
    fn an_ingest_that_times_out_is_spooled_not_refused_and_sent_again_once_the_host_answers() {
        let rig = stalled_rig();
        let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
        assert_eq!(code, 0, "{err}");
        assert!(err.contains("ledger-host timed out; spooled ingest to ") && !err.contains("refused"), "{err}");
        assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".jsonl")).count(), 1);
        fs::remove_file(rig.root.join("sleep")).unwrap();
        assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
        assert!(spooled(&rig).is_empty());
        assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);
    }

    #[test]
    fn an_agent_event_that_times_out_is_spooled_and_sent_again_once_the_host_answers() {
        let rig = stalled_rig();
        let event = ["agent-event", "--session", "s", "--kind", "note", "--key", "k1"];
        let (code, err) = call(&rig, &event, b"");
        assert_eq!(code, 0, "{err}");
        assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".argv")).count(), 1, "a timed-out agent-event is kept for replay");
        fs::remove_file(rig.root.join("sleep")).unwrap();
        assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
        assert!(spooled(&rig).is_empty());
        assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.ends_with("north threads agent-event --session s --kind note --key k1")).count(), 2);
    }

    #[test]
    fn a_caller_that_may_not_read_native_items_is_served_no_snapshot_pages() {
        let rig = rig(&[("THREADS_NO_NATIVE", "1")]);
        assert_eq!(call(&rig, &["snapshot-pages"], b""), (1, "NorthThreads: the snapshot holds native items: THREADS_NO_NATIVE is set (agy)\n".to_owned()));
    }

    #[test]
    fn a_busy_ledger_host_spools_the_batch_keeps_it_queued_and_stays_reachable_for_other_commands() {
        let rig = rig(&[]);
        let spooled = || {
            fs::read_dir(rig.root.join(".local/state/threads/spool")).unwrap()
                .filter_map(|entry| entry.unwrap().path().extension().map(|extension| extension.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
        };
        fs::write(rig.root.join("ingest-code"), ingest_lock::RETRY.to_string()).unwrap();
        let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
        assert_eq!(code, 0);
        assert!(err.contains("ledger-host busy; spooled ingest to "), "{err}");
        assert_eq!(spooled(), ["jsonl"]);
        let (code, err) = call(&rig, &["claim", "north#63"], b"");
        assert_eq!(code, 0, "a busy ingest lock is not an unreachable host: {err}");
        assert_eq!(spooled(), ["jsonl"], "a busy batch stays queued, never .failed");
        let (code, err) = call(&rig, &["ingest"], b"{\"n\": 2}\n");
        assert_eq!(code, 0);
        assert!(err.contains("spooled ingest to "), "{err}");
        assert_eq!(spooled(), ["jsonl", "jsonl"]);
        fs::remove_file(rig.root.join("ingest-code")).unwrap();
        assert_eq!(call(&rig, &["ingest"], b"{\"n\": 3}\n").0, 0);
        assert!(spooled().is_empty());
        let sent = log(&rig, "ingest.log");
        assert_eq!(&sent[sent.len() - 3..], ["{\"n\": 1}", "{\"n\": 2}", "{\"n\": 3}"]);
    }

    #[test]
    fn threads_client_reads_fall_back_to_the_snapshot_with_the_stale_line() {
        let rig = rig(&[]);
        fs::write(rig.root.join("code"), "255").unwrap();
        schema::ensure(&rig.client.snapshot).unwrap();
        rusqlite::Connection::open(&rig.client.snapshot).unwrap().execute("INSERT INTO job_runs VALUES ('nightly', 'laptop', '2026-11-05T00:00:00Z', NULL, NULL)", []).unwrap();
        File::options().write(true).open(&rig.client.snapshot).unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap();
        let (code, err) = call(&rig, &["job-runs"], b"");
        assert_eq!(code, 0);
        assert_eq!(err, "STALE: ledger snapshot from 2023-11-14T22:13:20Z; ledger-host unreachable\n");
        let (code, err) = call(&rig, &["claim", "north#63"], b"");
        assert_eq!(code, 255);
        assert_eq!(err, "NorthThreads: ledger-host unreachable; the ledger lives there, so nothing was read or recorded\n");
        let held: i64 = rusqlite::Connection::open(&rig.client.snapshot).unwrap().query_row("SELECT count(*) FROM claims", [], |row| row.get(0)).unwrap();
        assert_eq!(held, 0, "an unreachable host records nothing in the snapshot");
    }
    fn ledger_host(wait: Duration) -> (tempfile::TempDir, Client) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let variables = BTreeMap::from([
            ("HOME".to_owned(), root.display().to_string()),
            ("THREADS_DB".to_owned(), root.join("threads.db").display().to_string()),
        ]);
        let mut client = Client::new(root, variables, vec!["ssh".into()], unread_fleet());
        client.ingest_wait = wait;
        (directory, client)
    }

    fn batch(id: &str) -> Vec<u8> {
        format!(
            "{{\"id\":\"{id}\",\"role\":\"worker\",\"tier\":\"medium\",\"status\":\"finished\",\"outcome\":\"done\",\"started\":\"2026-10-26T11:00:00Z\",\"ended\":\"2026-10-26T11:05:00Z\",\"actual_min\":5,\"brief\":\"Item: makgora#459 x\\nCategory: feature\"}}\n"
        ).into_bytes()
    }

    fn recorded(root: &Path) -> Vec<String> {
        let db = rusqlite::Connection::open(root.join("threads.db")).unwrap();
        let mut statement = db.prepare("SELECT agent FROM runs ORDER BY agent").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn two_ingests_started_together_run_one_after_the_other_and_each_batch_is_recorded_once() {
        let (directory, client) = ledger_host(Duration::from_secs(30));
        let ingest = |id: &'static str| {
            let client = &client;
            move || {
                let mut err = Vec::new();
                let code = client.main(&["ingest".to_owned()], &mut &batch(id)[..], &mut err).unwrap();
                (code, String::from_utf8(err).unwrap())
            }
        };
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(ingest("one"));
            let second = scope.spawn(ingest("two"));
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!((first.0, second.0), (0, 0), "{first:?} {second:?}");
        let millis = |line: &Value, field: &str| Timestamp::parse_loose(line[field].as_str().unwrap()).unwrap().unix_millis();
        let mut holds: Vec<(i64, i64)> = logged(directory.path()).iter().map(|line| (millis(line, "ended") - line["held_ms"].as_i64().unwrap(), millis(line, "ended"))).collect();
        holds.sort_unstable();
        assert!(holds.len() == 2 && holds[0].1 <= holds[1].0, "the two ingests held the ledger at once: {holds:?}");
        assert_eq!(recorded(directory.path()), ["one", "two"]);
    }

    #[test]
    fn an_ingest_stalled_past_its_hold_cap_is_stopped_uncommitted_with_the_retry_code_and_its_retry_records_each_row_once() {
        let (directory, mut client) = ledger_host(Duration::from_secs(5));
        client.ingest_hold = Duration::from_secs(3);
        let writer = rusqlite::Connection::open(directory.path().join("threads.db")).unwrap();
        writer.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE").unwrap();
        let mut err = Vec::new();
        let begun = Instant::now();
        let code = client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut err).unwrap();
        let took = begun.elapsed();
        assert_eq!(code, ingest_lock::RETRY, "{}", String::from_utf8_lossy(&err));
        assert!(took >= Duration::from_secs(3) && took < Duration::from_secs(30), "{took:?}");
        assert_eq!(String::from_utf8(err).unwrap(), "NorthThreads: ingest held the ledger past 3 s; stopped it uncommitted, retry\n");
        assert!(ingest_lock::acquire(&directory.path().join("ingest.lock"), Duration::ZERO).unwrap().is_some());
        writer.execute_batch("ROLLBACK").unwrap();
        drop(writer);
        let lines = logged(directory.path());
        assert_eq!((lines[0]["exit"].as_i64(), lines[0]["held_ms"].as_i64().is_some_and(|held| held < 30_000)), (Some(i64::from(ingest_lock::RETRY)), true), "{lines:?}");
        for _ in 0..2 {
            assert_eq!(client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut Vec::new()).unwrap(), 0);
        }
        assert_eq!(recorded(directory.path()), ["one", "two"]);
    }

    fn logged(root: &Path) -> Vec<Value> {
        fs::read_to_string(root.join("ingest.log")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

}
