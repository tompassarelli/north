use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::threads::{board, causes, claims, hold, incidents, ingest_lock, ingest_log, items, jobs, landing, outside, runs, schema, snapshot, staffing, verb};
use crate::foundation::clock::Timestamp;
use crate::foundation::exit::{TIMED_OUT_STATUS, TRANSPORT_FAILED_STATUS};

mod process;
mod remote;
mod snapshot_pull;
pub mod spool;
pub use process::shell_quote;
use process::{Input, agent_event_checks, detach, has_usage};

const READS: [&str; 11] = [
    "job-runs", "show", "ready", "list", "holder", "unowned", "run-ids", "summary", "recommend", "calibrate", "opencode",
];
/// Each family of ledger verbs and the function that runs it; `claim` and `list` run through claims.
const PORTED: [verb::Family; 8] = [
    (&jobs::VERBS, jobs::run),
    (&items::VERBS, items::run),
    (&board::VERBS, board::run),
    (&staffing::VERBS, staffing::run),
    (&runs::VERBS, runs::run),
    (&landing::VERBS, landing::run),
    (&incidents::VERBS, incidents::run),
    (&causes::VERBS, causes::run),
];
/// The verb a call starts detached to send the spool to the ledger host without waiting on it.
const SPOOL_FLUSH: &str = "spool-flush";
/// Set on a `queue follow` the sender has vetted against its own checkouts before forwarding it.
const FORWARDED: &str = "THREADS_FORWARDED";
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_WAIT: Duration = Duration::from_secs(10);
const INGEST_WAIT: Duration = Duration::from_secs(5);
const INGEST_HOLD: Duration = Duration::from_secs(12);
/// How many rows one ingest commits under one hold of the ledger: a backfill of about 5,000 rows held it 33 to 45 s on 2026-11-07, over the 30 s a hold may last, and the hold cap only trips on a busy ledger, so a batch is cut here instead.
const INGEST_CHUNK_ROWS: usize = 400;
const SPOOL_DRAIN: Duration = Duration::from_secs(10);
/// How old a snapshot may be before an ingest carrying usage pulls it: each pull rebuilds the whole ledger image on the ledger host, and every host ingests usage each minute.
const USAGE_SNAPSHOT_AGE: Duration = Duration::from_secs(300);

/// `data` cut after every `rows` lines, whole lines only; a batch with no row is one empty chunk.
fn rows_in_chunks(data: &[u8], rows: usize) -> Vec<&[u8]> {
    let mut chunks = Vec::new();
    let (mut start, mut seen) = (0, 0);
    for (at, byte) in data.iter().enumerate() {
        if *byte == b'\n' {
            seen += 1;
            if seen == rows {
                chunks.push(&data[start..=at]);
                (start, seen) = (at + 1, 0);
            }
        }
    }
    if start < data.len() || chunks.is_empty() {
        chunks.push(&data[start..]);
    }
    chunks
}

/// The reads a remote client answers from its own snapshot, never the ledger host: each fetches every staffed repo's
/// issues from GitHub or scores every run, which on 2026-11-07 held nexus at 85% of a core per peer call.
fn snapshot_answers(argv: &[String]) -> bool {
    match argv.first().map(String::as_str) {
        Some("unowned") => true,
        Some("recommend") => recommend_records_nothing(&argv[1..]),
        _ => false,
    }
}

/// Whether a `recommend` call reads alone: one that names `--launched` or an Item without Follows records a prediction.
fn recommend_records_nothing(argv: &[String]) -> bool {
    let Ok(args) = verb::Args::parse(argv, &["--since", "--launched"], &[]) else { return false };
    let [facets] = args.positional.as_slice() else { return false };
    let fields = runs::brief::Fields::read(facets);
    args.get("--launched").is_none() && (fields.item.is_none() || fields.follows.is_some())
}

fn unanswered(code: i32) -> &'static str {
    if code == TIMED_OUT_STATUS { "ledger host timed out" } else { "ledger host unreachable" }
}

fn keyed_agent_event(argv: &[String], err: &mut dyn Write) -> NorthResult<Option<Vec<String>>> {
    if let Err(message) = agent_event_checks(argv) {
        writeln!(err, "NorthThreads: {message}")?;
        return Ok(None);
    }
    let mut keyed = argv.to_vec();
    if !argv.iter().any(|arg| arg == "--key" || arg.starts_with("--key=")) {
        let key = fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned();
        keyed.extend(["--key".to_owned(), key]);
    }
    Ok(Some(keyed))
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
    ingest_chunk_rows: usize,
    spool_drain: Duration,
    /// The `north threads` a call starts without waiting on it to pull a stale snapshot or send the spool; with none, the call does both inline.
    background: Option<Vec<OsString>>,
    /// The verb families a caller above this crate adds to the ones the ledger runs itself.
    families: &'static [verb::Family],
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
}

#[derive(Clone, Copy, PartialEq)]
enum Flush {
    Clear,
    Busy,
    Down,
}

pub fn run(arguments: &[String], fleet: Arc<dyn outside::FleetView>, families: &'static [verb::Family]) -> u8 {
    run_in(arguments, ambient_env(), fleet, families)
}

pub fn run_in(arguments: &[String], env: BTreeMap<String, String>, fleet: Arc<dyn outside::FleetView>, families: &'static [verb::Family]) -> u8 {
    let Some(mut client) = client_in(env, fleet) else {
        eprintln!("NorthThreads: no home directory");
        return 1;
    };
    client.families = families;
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
        let _ = client.refresh_snapshot(destination.trim(), Duration::ZERO);
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

/// Queues `words` as a write for the ledger host and starts a detached send, so a caller that stopped waiting on the ledger still has its write land once the host answers; returns the write's name in the spool, `None` when this host holds the ledger itself and has no spool to queue it in.
pub fn spool_write(words: &[&str]) -> NorthResult<Option<String>> {
    let Some(client) = ambient_client(unread_fleet()) else { return Ok(None) };
    if client.var("THREADS_DB").is_some() || !client.remote.exists() {
        return Ok(None);
    }
    let argv: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let name = client.spool_argv(&argv, "the caller's bound passed before the ledger host answered")?;
    if let Some(program) = &client.background {
        let dest = fs::read_to_string(&client.remote)?;
        detach(program, &[SPOOL_FLUSH.as_ref(), dest.trim().as_ref()]);
    }
    Ok(Some(name))
}

/// What the ledger host said when it refused the write [`spool_write`] queued as `name`; `None` while it waits, once it landed, or when the spool cannot be read.
pub fn spooled_refusal(name: &str) -> Option<String> {
    ambient_client(unread_fleet())?.spooled_refusal(name)
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

/// True when `THREADS_DB` names a ledger of this process's own, such as a test launch's scratch root, instead of the shared one.
pub fn private_ledger() -> bool {
    ambient_client(unread_fleet()).is_some_and(|client| client.var("THREADS_DB").is_some())
}

pub fn ledger_path() -> NorthResult<PathBuf> {
    ambient_client(unread_fleet()).map(|client| client.ledger()).ok_or_else(|| NorthError::Usage("no home directory".into()))
}

/// The name of the host the ledger lives on, the destination `~/.config/threads/remote` names without its user; None when this host holds the ledger itself.
pub fn ledger_host_name() -> Option<String> {
    let client = ambient_client(unread_fleet())?;
    if client.var("THREADS_DB").is_some() {
        return None;
    }
    let destination = fs::read_to_string(&client.remote).ok()?;
    destination.trim().rsplit('@').next().filter(|name| !name.is_empty()).map(str::to_owned)
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
            ingest_chunk_rows: INGEST_CHUNK_ROWS,
            spool_drain: SPOOL_DRAIN,
            background: Some(vec![crate::platform::north_binary::north_binary().into(), "threads".into()]),
            clock: Arc::new(Timestamp::now),
            families: &[],
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
        if let Some(dest) = self.ledger_remote()? {
            if snapshot_answers(argv) && self.snapshot.exists() {
                self.refresh_snapshot_later(&dest, snapshot::SNAPSHOT_AGE);
                return self.local(argv, Some(&self.snapshot), replay);
            }
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
        for chunk in rows_in_chunks(&data, self.ingest_chunk_rows) {
            let code = self.ingest_chunk(argv, chunk, err)?;
            if code != 0 {
                return Ok(code);
            }
        }
        Ok(0)
    }

    fn ingest_chunk(&self, argv: &[String], data: &[u8], err: &mut dyn Write) -> NorthResult<i32> {
        let ledger = self.ledger();
        let started = (self.clock)();
        let log = ledger.with_file_name("ingest.log");
        let record = |acquired: Option<Timestamp>, exit: i32| {
            let _ = ingest_log::append(&log, &ingest_log::Ingest { started, acquired, ended: (self.clock)(), exit, rows: ingest_log::rows(data) });
        };
        let Some(_lock) = ingest_lock::acquire(&ledger.with_file_name("ingest.lock"), self.ingest_wait)? else {
            writeln!(err, "NorthThreads: another ingest holds the ledger; retry")?;
            record(None, ingest_lock::RETRY);
            return Ok(ingest_lock::RETRY);
        };
        let acquired = (self.clock)();
        let hold = hold::Hold::start(self.ingest_hold);
        let outcome = self.local(argv, Some(&ledger), Some(data));
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
        let ported = PORTED.iter().chain(self.families).find(|(verbs, _)| verbs.contains(&verb)).map(|(_, run)| *run);
        let claim = claims::verbs::VERBS.contains(&verb);
        if ported.is_none() && !claim {
            let known: Vec<&str> = claims::verbs::VERBS.iter().chain(PORTED.iter().chain(self.families).flat_map(|(verbs, _)| verbs.iter())).copied().chain(["snapshot-pages", "snapshot-pull", SPOOL_FLUSH]).collect();
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
        let mut argv = argv.to_vec();
        if first == Some("agent-event") {
            let Some(keyed) = keyed_agent_event(&argv, err)? else { return Ok(Some(1)) };
            argv = keyed;
        }
        if first == Some("job-run") {
            return self.proxy_job_run(dest, &argv, err).map(Some);
        }
        let flush = self.flush_spool_for(first, dest, err)?;
        if first == Some("ingest") {
            return self.proxy_ingest(dest, &argv, stdin, flush, err).map(Some);
        }
        if first == Some("agent-event") {
            return self.proxy_agent_event(dest, &argv, flush, err).map(Some);
        }
        let reachable = flush != Flush::Down;
        let follow = first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow");
        let input = if follow {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            Input::Bytes(brief)
        } else {
            Input::Inherit
        };
        let code = if reachable { self.ssh(dest, &argv, input, false, err)? } else { TRANSPORT_FAILED_STATUS };
        if first == Some("agent-events") || first == Some("agent-event-state") {
            if code == TRANSPORT_FAILED_STATUS {
                writeln!(err, "NorthThreads: {dest} unreachable; canonical events unavailable")?;
            }
            return Ok(Some(code));
        }
        if code == TIMED_OUT_STATUS {
            return Ok(Some(code));
        }
        if code != TRANSPORT_FAILED_STATUS {
            self.refresh_after_answer(dest, &argv, code)?;
            return Ok(Some(code));
        }
        if first.is_some_and(|verb| READS.contains(&verb)) && self.snapshot.exists() {
            writeln!(err, "{}", self.stale_line(dest)?)?;
            return Ok(None);
        }
        writeln!(err, "NorthThreads: {dest} unreachable; the ledger lives there, so nothing was read or recorded")?;
        Ok(Some(code))
    }

    fn proxy_job_run(&self, dest: &str, argv: &[String], err: &mut dyn Write) -> NorthResult<i32> {
        let code = self.ssh(dest, argv, Input::Null, false, err)?;
        self.spool_if_unanswered(argv, code)
    }

    fn proxy_ingest(&self, dest: &str, argv: &[String], stdin: &mut dyn Read, flush: Flush, err: &mut dyn Write) -> NorthResult<i32> {
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        let mut state = if flush != Flush::Down { "busy" } else { "unreachable" };
        if flush == Flush::Clear {
            let code = self.ssh(dest, argv, Input::Bytes(data.clone()), false, err)?;
            if code == ingest_lock::RETRY {
                state = "busy";
            } else if code == TIMED_OUT_STATUS {
                state = "timed out";
            } else if code != TRANSPORT_FAILED_STATUS {
                if code == 0 && has_usage(&data)? {
                    self.refresh_snapshot_later(dest, USAGE_SNAPSHOT_AGE);
                }
                return Ok(code);
            }
        }
        let path = self.write_spool(&data, &format!("ledger host {state}"))?;
        writeln!(err, "NorthThreads: {dest} {state}; spooled ingest to {}", path.display())?;
        Ok(0)
    }

    fn proxy_agent_event(&self, dest: &str, argv: &[String], flush: Flush, err: &mut dyn Write) -> NorthResult<i32> {
        let code = if flush == Flush::Clear { self.ssh(dest, argv, Input::Null, false, err)? } else { TRANSPORT_FAILED_STATUS };
        self.spool_if_unanswered(argv, code)
    }

    fn spool_if_unanswered(&self, argv: &[String], code: i32) -> NorthResult<i32> {
        if !matches!(code, TRANSPORT_FAILED_STATUS | TIMED_OUT_STATUS) {
            return Ok(code);
        }
        self.spool_argv(argv, unanswered(code))?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests;
