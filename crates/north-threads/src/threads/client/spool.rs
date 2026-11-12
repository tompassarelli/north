use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::ops::ControlFlow;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use north_base::lifecycle::{Lifecycle, Lost, Moved, Refusal, State, Store};
use north_publish::publish;

use super::process::{Input, detach, python_json};
use super::{Client, Flush, SPOOL_FLUSH};
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::foundation::exit::{TIMED_OUT_STATUS, TRANSPORT_FAILED_STATUS};
use crate::threads::ingest_lock;
use withheld::{WILL_NOT_RUN, exited};

mod withheld;

const NOT_EXECUTABLE_STATUS: i32 = 126;
const COMMAND_NOT_FOUND_STATUS: i32 = 127;

/// Where a write spooled on a mirrored client stands on its way to the ledger host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Queued,
    Sending,
    Sent,
    Refused,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Queued, Self::Sending, Self::Sent, Self::Refused];

    fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Refused => "refused",
        }
    }
}

/// One spooled write, named by its file in the spool directory.
pub struct SpooledWrite;

struct Sent {
    code: i32,
    verb: Option<String>,
    stderr: String,
}

impl Lifecycle for SpooledWrite {
    const NAME: &'static str = "threads-spool";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        matches!(
            (from, to),
            (None | Some(Phase::Refused), Phase::Queued)
                | (None, Phase::Refused)
                | (Some(Phase::Queued), Phase::Sending)
                | (Some(Phase::Sending), Phase::Queued | Phase::Sent | Phase::Refused)
        )
    }

    fn dwell(state: Phase) -> Option<Duration> {
        match state {
            Phase::Queued => Some(Duration::from_secs(15 * 60)),
            Phase::Sending => Some(Duration::from_secs(10 * 60)),
            Phase::Refused => Some(Duration::ZERO),
            Phase::Sent => None,
        }
    }

    fn keep(state: Phase) -> Option<Duration> {
        match state {
            Phase::Sent => Some(Duration::from_secs(24 * 3600)),
            Phase::Refused => Some(Duration::from_secs(7 * 24 * 3600)),
            Phase::Queued | Phase::Sending => None,
        }
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Sending).then_some(Phase::Queued)
    }
}

/// The phase a spool file left by the marker-and-suffix scheme was in: `.jsonl` and `.argv` were waiting, `.failed`
/// was refused and never retried, and the flush lock and an unfinished `.tmp` are not writes.
fn adopted_phase(name: &str) -> Option<Phase> {
    match name.rsplit_once('.')?.1 {
        "jsonl" | "argv" => Some(Phase::Queued),
        "failed" => Some(Phase::Refused),
        _ => None,
    }
}

pub(super) struct Spool {
    store: Store,
    directory: PathBuf,
}

impl Spool {
    /// The spool at `directory` with its records in `ledger`; a file with no record is adopted from its suffix.
    pub(super) fn open(directory: &Path, ledger: &Path, at: Timestamp) -> NorthResult<Self> {
        Self::adopting(Store::open(ledger)?, directory, at)
    }

    pub(super) fn adopting(store: Store, directory: &Path, at: Timestamp) -> NorthResult<Self> {
        let mut spool = Self { store, directory: directory.to_path_buf() };
        for (name, phase) in spool.unrecorded()? {
            spool.adopt(&name, phase, at)?;
        }
        Ok(spool)
    }

    /// The spool files that have no record, with the phase their suffix puts them in.
    fn unrecorded(&self) -> NorthResult<Vec<(String, Phase)>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&self.directory)?.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(phase) = adopted_phase(&name) else { continue };
            if self.store.current::<SpooledWrite>(&name)?.is_none() {
                found.push((name, phase));
            }
        }
        Ok(found)
    }

    /// Records `name`, found with no record; another process that recorded it first has adopted it already.
    fn adopt(&mut self, name: &str, phase: Phase, at: Timestamp) -> NorthResult<()> {
        let cause = format!("adopted from the spool file {name}, which had no record");
        match self.store.transition_from::<SpooledWrite>(name, None, phase, &cause, at)? {
            Ok(_) | Err(Lost::AlreadyMoved { .. }) => Ok(()),
            Err(Lost::Refused(refusal)) => Err(refused(name, &refusal)),
        }
    }

    /// The names of the writes waiting to be sent, oldest name first, a write whose flush died mid-send among them, and
    /// one write of each verb the ledger host withheld over `PROBE_AFTER` ago, sent to ask whether it runs the verb now.
    pub(super) fn queued(&mut self, at: Timestamp) -> NorthResult<Vec<String>> {
        self.probe_withheld(at)?;
        let mut names = Vec::new();
        for record in self.store.instances::<SpooledWrite>()? {
            let record = if record.state == Phase::Sending { self.store.settle::<SpooledWrite>(&record.instance, at)? } else { Some(record) };
            names.extend(record.filter(|record| record.state == Phase::Queued).map(|record| record.instance));
        }
        names.sort();
        Ok(names)
    }

    pub(super) fn queue(&mut self, name: &str, cause: &str, at: Timestamp) -> NorthResult<()> {
        self.go(name, Phase::Queued, cause, at)
    }

    /// Starts sending `name`; false while a live process already holds it.
    pub(super) fn take(&mut self, name: &str, at: Timestamp) -> NorthResult<bool> {
        match self.store.transition::<SpooledWrite>(name, Phase::Sending, "the spool flush took it", at)? {
            Ok(_) => Ok(true),
            Err(Refusal::Held { .. }) => Ok(false),
            Err(refusal) => Err(refused(name, &refusal)),
        }
    }

    pub(super) fn release(&mut self, name: &str, cause: &str, at: Timestamp) -> NorthResult<()> {
        self.go(name, Phase::Queued, cause, at)
    }

    /// Marks `name` sent and queues every write of its `verb` the ledger host withheld, since the host runs it now; returns their names.
    pub(super) fn deliver(&mut self, name: &str, verb: Option<&str>, at: Timestamp) -> NorthResult<Vec<String>> {
        self.go(name, Phase::Sent, "the ledger host took it", at)?;
        verb.map_or_else(|| Ok(Vec::new()), |verb| self.resend_withheld(verb, at))
    }

    pub(super) fn refuse(&mut self, name: &str, cause: &str, at: Timestamp) -> NorthResult<()> {
        self.go(name, Phase::Refused, cause, at)
    }

    fn go(&mut self, name: &str, to: Phase, cause: &str, at: Timestamp) -> NorthResult<()> {
        let moved: Moved<Phase> = self.store.transition::<SpooledWrite>(name, to, cause, at)?;
        moved.map(drop).map_err(|refusal| refused(name, &refusal))
    }
}

fn refused(name: &str, refusal: &Refusal<Phase>) -> NorthError {
    NorthError::State(format!("spooled write {name}: {refusal:?}"))
}


fn spool_stem() -> String {
    format!("{}-{}", Timestamp::now().unix_nanos(), std::process::id())
}

/// The records of the spool at `spool`, kept beside it rather than in the host's lifecycle ledger, which a sandboxed worker that spools a write may read but not write.
pub fn records_path(spool: &Path) -> PathBuf {
    spool.with_file_name("spool-records.db")
}

/// Puts the records database at `records` in WAL mode, which keeps its log between commits: a rollback journal is deleted at each commit, and a sandboxed worker may write beside its spool but not remove there.
fn write_ahead(records: &Path) -> NorthResult<()> {
    if let Some(parent) = records.parent() {
        fs::create_dir_all(parent)?;
    }
    rusqlite::Connection::open(records).and_then(|db| db.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))).map_err(|error| NorthError::State(format!("the spool records at {}: {error}", records.display())))
}

impl Client {
    fn spool_records(&self) -> NorthResult<Spool> {
        let records = records_path(&self.spool);
        write_ahead(&records)?;
        Spool::open(&self.spool, &records, (self.clock)())
    }

    fn queue_spooled(&self, path: &Path, contents: &[u8], mode: u32, cause: &str) -> NorthResult<()> {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.spool_records()?.queue(&name, cause, (self.clock)())?;
        Ok(publish(path, contents, mode)?)
    }

    pub(super) fn write_spool(&self, data: &[u8], cause: &str) -> NorthResult<PathBuf> {
        fs::create_dir_all(&self.spool)?;
        let path = self.spool.join(format!("{}.jsonl", spool_stem()));
        self.queue_spooled(&path, data, 0o666, cause)?;
        Ok(path)
    }

    pub(super) fn spool_argv(&self, argv: &[String], cause: &str) -> NorthResult<String> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.spool)?;
        fs::set_permissions(&self.spool, fs::Permissions::from_mode(0o700))?;
        let name = format!("{}.argv", spool_stem());
        self.queue_spooled(&self.spool.join(&name), python_json(argv).as_bytes(), 0o600, cause)?;
        Ok(name)
    }

    pub(super) fn spooled_refusal(&self, name: &str) -> Option<String> {
        let record = self.spool_records().ok()?.store.current::<SpooledWrite>(name).ok()??;
        (record.state == Phase::Refused).then_some(record.cause)
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
        self.send_queued(dest, err, budget)
    }

    /// Sends the spool after any flush already holding it, because that flush may have found the host down just before it came back; one waiter covers every call made while it waits, so a later one leaves.
    fn flush_spool_in_turn(&self, dest: &str, err: &mut dyn Write) -> NorthResult<Flush> {
        if !self.spool.is_dir() {
            return Ok(Flush::Clear);
        }
        let next = File::create(self.spool.join(".next"))?;
        match next.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(Flush::Busy),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        let lock = File::create(self.spool.join(".lock"))?;
        lock.lock()?;
        drop(next);
        self.send_queued(dest, err, None)
    }

    fn send_queued(&self, dest: &str, err: &mut dyn Write, budget: Option<Duration>) -> NorthResult<Flush> {
        let mut spool = self.spool_records()?;
        let started = Instant::now();
        let mut names = spool.queued((self.clock)())?;
        let mut next = 0;
        while let Some(name) = names.get(next).cloned() {
            next += 1;
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
            match self.send_and_settle(&mut spool, &name, &path, file, dest, err)? {
                ControlFlow::Break(flush) => return Ok(flush),
                ControlFlow::Continue(resent) => names.extend(resent),
            }
        }
        Ok(Flush::Clear)
    }

    /// Sends one write and records how the ledger host answered: stop the flush with how it stands, or go on with the withheld writes the delivery queued again.
    fn send_and_settle(&self, spool: &mut Spool, name: &str, path: &Path, file: File, dest: &str, err: &mut dyn Write) -> NorthResult<ControlFlow<Flush, Vec<String>>> {
        let sent = match self.send_spooled(path, file, dest, err) {
            Ok(sent) => sent,
            Err(error) => {
                spool.release(name, &format!("the flush failed before the ledger host answered: {error}"), (self.clock)())?;
                return Err(error);
            }
        };
        let code = sent.code;
        if code == TRANSPORT_FAILED_STATUS {
            spool.release(name, "ledger host unreachable", (self.clock)())?;
            return Ok(ControlFlow::Break(Flush::Down));
        }
        if matches!(code, ingest_lock::RETRY | TIMED_OUT_STATUS) {
            spool.release(name, if code == TIMED_OUT_STATUS { "ledger host timed out" } else { "ledger host busy" }, (self.clock)())?;
            return Ok(ControlFlow::Break(Flush::Busy));
        }
        let refused_for_good = code == NOT_EXECUTABLE_STATUS && sent.stderr.contains("remote-serve: refused");
        if let Some(verb) = sent.verb.as_deref().filter(|_| refused_for_good) {
            let why = sent.stderr.lines().find(|line| line.contains("remote-serve: refused")).map(str::trim).unwrap_or("the ledger host refused it");
            spool.refuse(name, &format!("{WILL_NOT_RUN}{verb}`: {why}"), (self.clock)())?;
            writeln!(err, "NorthThreads: the ledger host will not run `{verb}`; {} is sent again once it does", path.display())?;
            return Ok(ControlFlow::Continue(Vec::new()));
        }
        if matches!(code, NOT_EXECUTABLE_STATUS | COMMAND_NOT_FOUND_STATUS) {
            spool.release(name, &format!("the ledger host could not run the ingest (exit {code})"), (self.clock)())?;
            return Ok(ControlFlow::Break(Flush::Busy));
        }
        if code != 0 {
            let said = sent.stderr.lines().map(str::trim).rfind(|line| !line.is_empty());
            spool.refuse(name, &said.map_or_else(|| exited(code), |said| format!("{}: {said}", exited(code))), (self.clock)())?;
            writeln!(err, "NorthThreads: remote ingest refused {}; it will not be sent again", path.display())?;
            return Ok(ControlFlow::Continue(Vec::new()));
        }
        let resent = spool.deliver(name, sent.verb.as_deref(), (self.clock)())?;
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(ControlFlow::Continue(resent)),
        }
    }

    fn send_spooled(&self, path: &Path, mut file: File, dest: &str, err: &mut dyn Write) -> NorthResult<Sent> {
        if path.extension().is_some_and(|extension| extension == "argv") {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let args: Vec<String> = serde_json::from_str(&text)?;
            let quiet = args.first().is_some_and(|verb| verb == "agent-event");
            let (code, stderr) = self.ssh_capturing(dest, &args, Input::Null, quiet, err)?;
            Ok(Sent { code, verb: args.into_iter().next(), stderr })
        } else {
            let (code, stderr) = self.ssh_capturing(dest, &["ingest".to_owned()], Input::File(file), false, err)?;
            Ok(Sent { code, verb: None, stderr })
        }
    }

    /// A spooled write sends the spool before itself, because its order behind the spool matters; any other call leaves the spool to a detached `spool-flush`, because one spooled usage batch holds the ledger host for seconds and a bounded call such as a launch's `holder` must not wait on them.
    pub(super) fn flush_spool_for(&self, verb: Option<&str>, dest: &str, err: &mut dyn Write) -> NorthResult<Flush> {
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
        Ok(!self.spool_records()?.queued((self.clock)())?.is_empty())
    }

    pub(super) fn spool_flush_verb(&self, argv: &[String], err: &mut dyn Write) -> NorthResult<i32> {
        let [_, dest] = argv else {
            return Err(NorthError::Usage(format!("usage: north threads {SPOOL_FLUSH} DEST")));
        };
        Ok(match self.flush_spool_in_turn(dest, err)? {
            Flush::Clear => 0,
            Flush::Busy => ingest_lock::RETRY,
            Flush::Down => TRANSPORT_FAILED_STATUS,
        })
    }
}

#[cfg(test)]
mod spool_lifecycle {
    use north_base::lifecycle::{FaultKind, Liveness, Owner};

    use super::*;

    struct Dead(u32);

    impl Liveness for Dead {
        fn alive(&self, owner: &Owner) -> Option<bool> {
            (owner.host == "client").then_some(owner.pid != self.0)
        }
    }

    fn store(path: &Path, pid: u32, dead: u32) -> Store {
        Store::open_as(path, Owner { host: "client".into(), pid, started: 1, booted_ms: 1 }, Box::new(Dead(dead))).unwrap()
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_800_000_000 + seconds)
    }

    fn rig() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let spool = directory.path().join("spool");
        std::fs::create_dir(&spool).unwrap();
        let ledger = directory.path().join("ledger.db");
        (directory, spool, ledger)
    }

    #[test]
    fn a_sent_write_cannot_be_queued_again_and_health_sees_the_attempt() {
        let (_keep, directory, ledger) = rig();
        let mut spool = Spool::adopting(store(&ledger, 10, 0), &directory, at(0)).unwrap();
        spool.queue("1-1.jsonl", "the ledger host was unreachable", at(0)).unwrap();
        assert!(spool.take("1-1.jsonl", at(1)).unwrap());
        spool.deliver("1-1.jsonl", None, at(2)).unwrap();

        assert!(spool.queue("1-1.jsonl", "a writer that reused the name", at(3)).is_err());

        let record = spool.store.current::<SpooledWrite>("1-1.jsonl").unwrap().unwrap();
        assert_eq!((record.state, record.seq), (Phase::Sent, 3));
        let faults = spool.store.faults(at(4)).unwrap();
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].kind, FaultKind::Illegal { to: "queued".into(), at_ms: at(3).unix_millis() });
    }

    #[test]
    fn a_flush_killed_mid_send_is_reported_and_the_next_flush_logs_its_death_and_resends() {
        let (_keep, directory, ledger) = rig();
        std::fs::write(directory.join("2-2.argv"), "[]").unwrap();
        std::fs::write(directory.join("3-3.failed"), "{}").unwrap();
        std::fs::write(directory.join(".lock"), "").unwrap();
        let mut killed = Spool::adopting(store(&ledger, 10, 0), &directory, at(0)).unwrap();
        assert_eq!(killed.queued(at(0)).unwrap(), ["2-2.argv"]);
        assert!(killed.take("2-2.argv", at(1)).unwrap());

        let mut next = Spool::adopting(store(&ledger, 11, 10), &directory, at(2)).unwrap();
        let faults = next.store.faults(at(2)).unwrap();
        assert_eq!(faults.iter().map(|fault| (fault.instance.as_str(), fault.state.as_str())).collect::<Vec<_>>(), [("3-3.failed", "refused"), ("2-2.argv", "sending")]);
        assert!(matches!(faults[0].kind, FaultKind::Overdue { .. }) && matches!(faults[1].kind, FaultKind::Orphaned { .. }));

        assert_eq!(next.queued(at(3)).unwrap(), ["2-2.argv"]);
        assert!(next.take("2-2.argv", at(3)).unwrap());
        let causes: Vec<_> = next.store.history(SpooledWrite::NAME, "2-2.argv").unwrap().into_iter().map(|logged| (logged.to, logged.cause)).collect();
        assert_eq!(causes[2], ("queued".to_owned(), "the holder, pid 10 on client, is gone".to_owned()));
        assert_eq!(next.store.current::<SpooledWrite>("3-3.failed").unwrap().unwrap().state, Phase::Refused);
    }

    #[test]
    fn a_lost_race_to_adopt_a_spool_file_is_not_an_illegal_move() {
        let (_keep, directory, ledger) = rig();
        std::fs::write(directory.join("4-4.jsonl"), "{}").unwrap();
        let mut first = Spool { store: store(&ledger, 10, 0), directory: directory.clone() };
        let mut second = Spool { store: store(&ledger, 11, 0), directory };
        let (seen_first, seen_second) = (first.unrecorded().unwrap(), second.unrecorded().unwrap());
        assert_eq!((seen_first.len(), seen_second.len()), (1, 1));

        first.adopt("4-4.jsonl", Phase::Queued, at(0)).unwrap();
        second.adopt("4-4.jsonl", Phase::Queued, at(1)).unwrap();

        let record = second.store.current::<SpooledWrite>("4-4.jsonl").unwrap().unwrap();
        assert_eq!((record.state, record.seq), (Phase::Queued, 1));
        assert!(second.store.history(SpooledWrite::NAME, "4-4.jsonl").unwrap().iter().all(|logged| !logged.refused));
        assert!(second.store.faults(at(2)).unwrap().is_empty());
    }
}
