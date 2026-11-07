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
    let far = far_hosts(&cfg, fleet.host());
    let pulled: Vec<NorthResult<Snapshot>> = std::thread::scope(|scope| {
        let running: Vec<_> = far.iter().map(|(_, ssh)| scope.spawn(|| OverSsh::default().api_spend(ssh))).collect();
        running.into_iter().map(|run| run.join().unwrap_or_else(|_| Err(NorthError::State("the pull panicked".into())))).collect()
    });
    for ((name, _), pulled) in far.iter().zip(pulled) {
        let stored = pulled.map_err(|error| error.user_message()).and_then(|snapshot| credit::store(&ledger(), &Snapshot { host: name.clone(), ..snapshot }));
        if let Err(error) = stored {
            failed.push(format!("api spend of {name}: {error}"));
        }
    }
    failed
}

/// Every `[hosts.NAME]` but this host's that declares an `ssh` or `deploy_ssh` destination, as (name, destination).
fn far_hosts(cfg: &toml_edit::DocumentMut, here: &str) -> Vec<(String, String)> {
    let hosts = cfg.get("hosts").and_then(toml_edit::Item::as_table_like);
    let declared = hosts.into_iter().flat_map(|hosts| hosts.iter());
    declared.filter(|(name, _)| *name != here).filter_map(|(name, host)| Some((name.to_owned(), gate::ssh_destination(["ssh", "deploy_ssh"].iter().find_map(|key| host.get(key)?.as_str())?)?.to_owned()))).collect()
}

#[cfg(test)]
mod tests;
