use crate::fleet::registry::rewritten::Rewrites;
use crate::threads::claims::ledger;
use crate::threads::client::threads_database;
use crate::threads::rewrites;

/// Reads the 24-hour closer's rewrites from this host's threads ledger or its snapshot of the ledger host's.
pub(super) struct Ledger;

impl Rewrites for Ledger {
    fn rewrites(&self) -> Vec<(String, String)> {
        ledger::open(&threads_database(), false).ok().and_then(|db| rewrites::all(&db).ok()).unwrap_or_default()
    }
}
