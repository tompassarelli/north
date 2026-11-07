//! The api spend this host reports to the others, and the pull of every other host's report on the fleet tick,
//! so `north usage` and the api pool charge a grant with the runs of every host.
use north_core::credit::{self, Snapshot};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::OverSsh;
use crate::pacing::placement::{config, gate};
use crate::watch;

fn ledger() -> std::path::PathBuf {
    watch::state_home().join("north/ledger.db")
}

pub(super) fn take(fleet: &Fleet) -> NorthResult<Snapshot> {
    let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
    Snapshot::take(fleet.host(), &ledger(), fleet.open_api_turns()?, now).map_err(NorthError::State)
}

/// Stores this host's snapshot and each other `[hosts]` machine's, returning one line per host that could not be read.
pub(super) fn pull(fleet: &Fleet) -> Vec<String> {
    let mut failed = Vec::new();
    if let Err(error) = take(fleet).map_err(|error| error.user_message()).and_then(|snapshot| credit::store(&ledger(), &snapshot)) {
        failed.push(format!("api spend of {}: {error}", fleet.host()));
    }
    let Ok(cfg) = config() else { return failed };
    let far: Vec<_> = gate::machines(&cfg).into_iter().filter(|machine| machine.name != fleet.host()).collect();
    let pulled: Vec<NorthResult<Snapshot>> = std::thread::scope(|scope| {
        let running: Vec<_> = far.iter().map(|machine| scope.spawn(|| OverSsh::default().api_spend(&machine.ssh))).collect();
        running.into_iter().map(|run| run.join().unwrap_or_else(|_| Err(NorthError::State("the pull panicked".into())))).collect()
    });
    for (machine, pulled) in far.iter().zip(pulled) {
        let stored = pulled.map_err(|error| error.user_message()).and_then(|snapshot| credit::store(&ledger(), &Snapshot { host: machine.name.clone(), ..snapshot }));
        if let Err(error) = stored {
            failed.push(format!("api spend of {}: {error}", machine.name));
        }
    }
    failed
}
