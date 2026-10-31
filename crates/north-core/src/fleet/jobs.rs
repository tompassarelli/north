//! Whether a `job-run --notify` job still running will send its exit to a session: the job's JOB-START
//! row registers a wait with a lease on the ledger host's clock, each JOB-BEAT extends it and JOB-EXIT settles it.
use std::collections::BTreeSet;

pub trait JobLeases: Send + Sync {
    fn leased(&self, session: &str) -> bool;
}

#[derive(Default)]
pub struct HeldLeases(pub BTreeSet<String>);

impl JobLeases for HeldLeases {
    fn leased(&self, session: &str) -> bool {
        self.0.contains(session)
    }
}
