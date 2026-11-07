use std::path::Path;
use std::time::Duration;

use north_base::lifecycle::{Lifecycle, Moved, Refusal, State, Store};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

/// Where a write spooled on a mirrored client stands on its way to the ledger host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
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
pub(super) struct SpooledWrite;

impl Lifecycle for SpooledWrite {
    const NAME: &'static str = "threads-spool";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        matches!(
            (from, to),
            (None, Phase::Queued | Phase::Refused) | (Some(Phase::Queued), Phase::Sending) | (Some(Phase::Sending), Phase::Queued | Phase::Sent | Phase::Refused)
        )
    }

    fn dwell(state: Phase) -> Option<Duration> {
        match state {
            Phase::Queued => Some(Duration::from_secs(15 * 60)),
            Phase::Sending => Some(Duration::from_secs(10 * 60)),
            Phase::Sent | Phase::Refused => None,
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
}

impl Spool {
    /// The spool at `directory` with its records in `ledger`; a file with no record is adopted from its suffix.
    pub(super) fn open(directory: &Path, ledger: &Path, at: Timestamp) -> NorthResult<Self> {
        Self::adopting(Store::open(ledger)?, directory, at)
    }

    pub(super) fn adopting(store: Store, directory: &Path, at: Timestamp) -> NorthResult<Self> {
        let mut spool = Self { store };
        for entry in std::fs::read_dir(directory)?.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(phase) = adopted_phase(&name) else { continue };
            if spool.store.current::<SpooledWrite>(&name)?.is_none() {
                spool.go(&name, phase, &format!("adopted from the spool file {name}, which had no record"), at)?;
            }
        }
        Ok(spool)
    }

    /// The names of the writes waiting to be sent, oldest name first.
    pub(super) fn queued(&self) -> NorthResult<Vec<String>> {
        let mut names: Vec<String> = self.store.instances::<SpooledWrite>()?.into_iter().filter(|record| record.state == Phase::Queued).map(|record| record.instance).collect();
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

    pub(super) fn deliver(&mut self, name: &str, at: Timestamp) -> NorthResult<()> {
        self.go(name, Phase::Sent, "the ledger host took it", at)
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
        spool.deliver("1-1.jsonl", at(2)).unwrap();

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
        assert_eq!(killed.queued().unwrap(), ["2-2.argv"]);
        assert!(killed.take("2-2.argv", at(1)).unwrap());

        let mut next = Spool::adopting(store(&ledger, 11, 10), &directory, at(2)).unwrap();
        let faults = next.store.faults(at(2)).unwrap();
        assert_eq!(faults.iter().map(|fault| (fault.instance.as_str(), fault.state.as_str())).collect::<Vec<_>>(), [("2-2.argv", "sending")]);
        assert!(matches!(faults[0].kind, FaultKind::Orphaned { .. }));

        assert!(next.take("2-2.argv", at(3)).unwrap());
        let causes: Vec<_> = next.store.history(SpooledWrite::NAME, "2-2.argv").unwrap().into_iter().map(|logged| (logged.to, logged.cause)).collect();
        assert_eq!(causes[2], ("queued".to_owned(), "the holder, pid 10 on client, is gone".to_owned()));
        assert_eq!(next.store.current::<SpooledWrite>("3-3.failed").unwrap().unwrap().state, Phase::Refused);
    }
}
