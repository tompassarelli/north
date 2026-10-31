//! Whether a `job-run --notify` job still running will send its exit to a session: the job's JOB-START
//! row registers a wait with a lease on the ledger host's clock, each JOB-BEAT extends it and JOB-EXIT settles it.
use std::collections::BTreeSet;

use super::store::Store;
use crate::threads::client;

const LEASED: &[&str] = &["north", "fleet", "row", "leased"];

pub trait JobLeases: Send + Sync {
    fn leased(&self, session: &str) -> bool;
}

pub struct LedgerLeases;

impl JobLeases for LedgerLeases {
    fn leased(&self, session: &str) -> bool {
        let words: Vec<&str> = LEASED.iter().copied().chain([session]).collect();
        let answered = match client::ask_ledger_host(&words, &[]) {
            Ok(Some(answer)) => Ok(String::from_utf8_lossy(&answer).trim() == "leased"),
            Ok(None) => client::ledger_path().and_then(|path| Store::open(&path)?.job_leased(session)),
            Err(error) => Err(error),
        };
        answered.unwrap_or_else(|error| {
            eprintln!("north fleet: the job leases of {session} were not read: {}", error.user_message());
            false
        })
    }
}

#[derive(Default)]
pub struct HeldLeases(pub BTreeSet<String>);

impl JobLeases for HeldLeases {
    fn leased(&self, session: &str) -> bool {
        self.0.contains(session)
    }
}
