//! Whether a `job-run --notify` job still running will send its exit to a session: the job's JOB-START
//! row registers a wait with a lease on the ledger host's clock, each JOB-BEAT extends it and JOB-EXIT settles it.
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::error::NorthResult;

use super::store::{FleetRow, RowKind, Spool};

pub trait JobLeases: Send + Sync {
    fn leased(&self, session: &str) -> NorthResult<bool>;
}

#[derive(Default)]
pub struct HeldLeases(pub BTreeSet<String>);

impl JobLeases for HeldLeases {
    fn leased(&self, session: &str) -> NorthResult<bool> {
        Ok(self.0.contains(session))
    }
}

pub struct SpooledLeases {
    ledger: Arc<dyn JobLeases>,
    spool: Spool,
}

impl SpooledLeases {
    pub fn new(ledger: Arc<dyn JobLeases>, spool: Spool) -> Self {
        Self { ledger, spool }
    }

    fn starting(&self, session: &str) -> NorthResult<bool> {
        let rows: Vec<FleetRow> = self.spool.listing()?.into_iter().filter_map(|spooled| serde_json::from_value(spooled.row).ok()).filter(|row: &FleetRow| row.to == session).collect();
        let key = |row: &FleetRow| row.job.as_ref().map(|job| job.key.clone());
        Ok(rows.iter().filter(|row| row.kind == RowKind::JobStart).filter_map(key).any(|started| !rows.iter().any(|row| row.kind == RowKind::JobExit && key(row).as_deref() == Some(started.as_str()))))
    }
}

impl JobLeases for SpooledLeases {
    fn leased(&self, session: &str) -> NorthResult<bool> {
        if self.starting(session)? {
            return Ok(true);
        }
        self.ledger.leased(session)
    }
}

#[cfg(test)]
mod tests {
    use crate::error::NorthError;
    use crate::fleet::store::JobPayload;

    use super::*;

    struct Ledger(Result<bool, &'static str>);

    impl JobLeases for Ledger {
        fn leased(&self, _session: &str) -> NorthResult<bool> {
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
            sender: None,
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
        assert!(leases.leased("worker").unwrap());
        assert!(!leases.leased("other").unwrap());
    }

    #[test]
    fn a_job_start_whose_exit_is_also_in_the_spool_no_longer_leases_its_session() {
        let rows = [row("1-a", RowKind::JobStart, "worker", "soak#1"), row("2-b", RowKind::JobExit, "worker", "soak#1")];
        assert!(!leases(Ok(false), &rows).1.leased("worker").unwrap());
    }

    #[test]
    fn the_exit_of_another_job_does_not_settle_a_start_in_the_spool() {
        let rows = [row("1-a", RowKind::JobStart, "worker", "soak#1"), row("2-b", RowKind::JobExit, "worker", "soak#2")];
        assert!(leases(Ok(false), &rows).1.leased("worker").unwrap());
    }

    #[test]
    fn a_spooled_job_start_leases_its_session_even_when_the_ledger_cannot_be_read() {
        assert!(leases(Err("down"), &[row("1-a", RowKind::JobStart, "worker", "soak#1")]).1.leased("worker").unwrap());
    }

    #[test]
    fn the_ledgers_answer_stands_when_the_spool_holds_no_job_start_and_its_error_is_passed_on() {
        assert!(leases(Ok(true), &[]).1.leased("worker").unwrap());
        assert!(!leases(Ok(false), &[row("1-a", RowKind::Message, "worker", "")]).1.leased("worker").unwrap());
        assert!(leases(Err("down"), &[]).1.leased("worker").is_err());
    }

    #[test]
    fn a_spooled_row_that_does_not_parse_is_not_a_job_start() {
        let (root, leases) = leases(Ok(false), &[]);
        std::fs::create_dir_all(root.path().join("spool")).unwrap();
        std::fs::write(root.path().join("spool/1-bad.json"), b"{not a row").unwrap();
        assert!(!leases.leased("worker").unwrap());
    }
}
