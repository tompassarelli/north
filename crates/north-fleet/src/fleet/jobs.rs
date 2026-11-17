//! Whether a `job-run --notify` job still running will send its exit to a session: the job's JOB-START
//! row registers a wait with a lease on the ledger host's clock, each JOB-BEAT extends it and JOB-EXIT settles it.
//! A JOB-START not yet in the ledger leases its session from the spool of this host or of the host the session is placed on.
use std::collections::BTreeSet;
use std::sync::Arc;

use north_foundation::error::NorthResult;

use super::model::Machine;
use super::store::{FleetRow, OverSsh, RowKind, Spool};

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
    far: Option<Arc<OverSsh>>,
}

impl SpooledLeases {
    pub fn new(ledger: Arc<dyn JobLeases>, spool: Spool) -> Self {
        Self { ledger, spool, far: None }
    }

    pub fn reading_far(mut self, far: Arc<OverSsh>) -> Self {
        self.far = Some(far);
        self
    }

    fn starting_here(&self, session: &str) -> NorthResult<bool> {
        let rows: Vec<FleetRow> = self.spool.listing()?.into_iter().filter_map(|spooled| serde_json::from_value(spooled.row).ok()).collect();
        Ok(starting(&rows, session))
    }

    fn starting_there(&self, session: &str, machine: &Machine) -> NorthResult<bool> {
        let Some(far) = &self.far else { return Ok(false) };
        Ok(starting(&far.take(machine.session_login())?.rows, session))
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
