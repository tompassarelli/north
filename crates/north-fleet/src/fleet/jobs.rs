//! Whether a `job-run --notify` job still running will send its exit to a session: the job's JOB-START
//! row registers a wait with a lease on the ledger host's clock, each JOB-BEAT extends it and JOB-EXIT settles it.
//! A JOB-START not yet in the ledger leases its session from the spool of this host or of the host the session is placed on.
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::error::NorthResult;

use super::model::Machine;
use super::store::{FarSpools, FleetRow, RowKind, Spool};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lease {
    Held,
    Free,
    Unknown,
}

impl Lease {
    pub fn read(answer: NorthResult<bool>) -> Self {
        match answer {
            Ok(true) => Self::Held,
            Ok(false) => Self::Free,
            Err(_) => Self::Unknown,
        }
    }
}

pub trait JobLeases: Send + Sync {
    fn leased(&self, session: &str, machine: Option<&Machine>) -> NorthResult<bool>;
}

#[derive(Default)]
pub struct HeldLeases(pub BTreeSet<String>);

impl JobLeases for HeldLeases {
    fn leased(&self, session: &str, _machine: Option<&Machine>) -> NorthResult<bool> {
        Ok(self.0.contains(session))
    }
}

pub struct SpooledLeases {
    ledger: Arc<dyn JobLeases>,
    spool: Spool,
    far: Option<Arc<dyn FarSpools + Send + Sync>>,
}

impl SpooledLeases {
    pub fn new(ledger: Arc<dyn JobLeases>, spool: Spool) -> Self {
        Self { ledger, spool, far: None }
    }

    pub fn reading_far(mut self, far: Arc<dyn FarSpools + Send + Sync>) -> Self {
        self.far = Some(far);
        self
    }

    fn starting_here(&self, session: &str) -> NorthResult<bool> {
        let rows: Vec<FleetRow> = self.spool.listing()?.into_iter().filter_map(|spooled| serde_json::from_value(spooled.row).ok()).collect();
        Ok(starting(&rows, session))
    }

    fn starting_there(&self, session: &str, machine: &Machine) -> NorthResult<bool> {
        let Some(far) = &self.far else { return Ok(false) };
        Ok(starting(&far.take(&machine.ssh)?.rows, session))
    }
}

fn starting(rows: &[FleetRow], session: &str) -> bool {
    let rows: Vec<&FleetRow> = rows.iter().filter(|row| row.to == session).collect();
    let key = |row: &&FleetRow| row.job.as_ref().map(|job| job.key.clone());
    rows.iter().filter(|row| row.kind == RowKind::JobStart).filter_map(key).any(|started| !rows.iter().any(|row| row.kind == RowKind::JobExit && key(row).as_deref() == Some(started.as_str())))
}

impl JobLeases for SpooledLeases {
    fn leased(&self, session: &str, machine: Option<&Machine>) -> NorthResult<bool> {
        if self.starting_here(session)? || self.ledger.leased(session, machine)? {
            return Ok(true);
        }
        match machine {
            Some(machine) => self.starting_there(session, machine),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::NorthError;
    use crate::fleet::sender::Sender;
    use crate::fleet::store::{FarRegistry, FleetTable, HandedRow, JobPayload, Pull};

    use super::*;

    struct Ledger(Result<bool, &'static str>);

    impl JobLeases for Ledger {
        fn leased(&self, _session: &str, _machine: Option<&Machine>) -> NorthResult<bool> {
            self.0.map_err(|why| NorthError::Refused(why.into()))
        }
    }

    fn row(id: &str, kind: RowKind, to: &str, key: &str) -> FleetRow {
        FleetRow {
            id: id.into(),
            kind,
            to: to.into(),
            from: "laptop".into(),
            urgent: false,
            sent_at: 1,
            text: String::new(),
            sender: Sender::Tom,
            gate: None,
            answer: None,
            job: Some(JobPayload { key: key.into(), lease_secs: None, exit: None }),
            origin: None,
        }
    }

    fn leases(ledger: Result<bool, &'static str>, rows: &[FleetRow]) -> (tempfile::TempDir, SpooledLeases) {
        let root = tempfile::tempdir().unwrap();
        let spool = Spool::at(root.path());
        rows.iter().for_each(|row| spool.put(row).unwrap());
        (root, SpooledLeases::new(Arc::new(Ledger(ledger)), spool))
    }

    #[test]
    fn a_job_start_still_in_this_hosts_spool_leases_its_session_while_the_ledger_says_free() {
        let (_root, leases) = leases(Ok(false), &[row("1-a", RowKind::JobStart, "worker", "soak#1")]);
        assert!(leases.leased("worker", None).unwrap());
        assert!(!leases.leased("other", None).unwrap());
    }

    struct Far(Vec<FleetRow>, Result<(), &'static str>);

    impl FarSpools for Far {
        fn take(&self, ssh: &str) -> NorthResult<Pull> {
            assert_eq!(ssh, "gex45");
            self.1.map_err(|why| NorthError::Refused(why.into()))?;
            Ok(Pull::from(self.0.clone()))
        }

        fn drop(&self, _: &str, _: &[String]) -> NorthResult<()> {
            Ok(())
        }

        fn set_aside(&self, _: &str, _: &[String]) -> NorthResult<()> {
            Ok(())
        }

        fn cache(&self, _: &str, _: &FleetTable) -> NorthResult<()> {
            Ok(())
        }

        fn registry(&self, _: &str) -> NorthResult<FarRegistry> {
            Ok(FarRegistry::default())
        }

        fn deliver(&self, _: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
            Ok(Vec::new())
        }
    }

    fn placed() -> Machine {
        Machine { name: "gex45".into(), ssh: "gex45".into() }
    }

    fn far_leases(ledger: Result<bool, &'static str>, far: Far) -> (tempfile::TempDir, SpooledLeases) {
        let (root, leases) = leases(ledger, &[]);
        (root, leases.reading_far(Arc::new(far)))
    }

    #[test]
    fn a_far_spool_that_cannot_be_read_is_an_error_not_a_free_session_unless_the_ledger_already_leases_it() {
        assert!(far_leases(Ok(false), Far(Vec::new(), Err("ssh down"))).1.leased("child", Some(&placed())).is_err());
        assert!(far_leases(Ok(true), Far(Vec::new(), Err("ssh down"))).1.leased("child", Some(&placed())).unwrap());
    }
}
