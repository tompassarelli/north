//! The api spend this host reports to the others, and the pull of every other host's report on the fleet tick,
//! so `north usage` and the api pool charge a grant with the runs of every host; and the swap of exhausted marks,
//! so an organization one host found out of credit is closed to launches on every host.
use std::collections::BTreeMap;
use std::path::PathBuf;

use north_core::credit::{self, Snapshot, marks};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::OverSsh;
use crate::foundation::clock::Timestamp;
use crate::pacing::placement::{config, gate};
use crate::watch;

type Marks = BTreeMap<String, String>;

fn ledger() -> PathBuf {
    watch::state_home().join("north/ledger.db")
}

fn marks_dir() -> PathBuf {
    watch::state_home().join("north/api-exhausted")
}

pub(super) fn take(fleet: &Fleet) -> NorthResult<Snapshot> {
    let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
    Snapshot::take(fleet.host(), &ledger(), fleet.open_api_turns()?, now).map_err(NorthError::State)
}

/// Adopts the exhausted marks another host `sent` and answers with every mark this host holds.
pub(super) fn swap_marks(sent: &Marks) -> NorthResult<Marks> {
    let today = Timestamp::now().iso_date();
    marks::adopt(&marks_dir(), sent, &today)?;
    Ok(marks::all(&marks_dir(), &today))
}

/// Stores this host's snapshot and each other `[hosts]` machine's and swaps exhausted marks with each, returning one line per host that could not be read and per mark adopted.
pub(super) fn pull(fleet: &Fleet) -> Vec<String> {
    let mut failed = Vec::new();
    if let Err(error) = take(fleet).map_err(|error| error.user_message()).and_then(|snapshot| credit::store(&ledger(), &snapshot)) {
        failed.push(format!("api spend of {}: {error}", fleet.host()));
    }
    let Ok(cfg) = config() else { return failed };
    let far = far_hosts(&cfg, fleet.host());
    let held = marks::all(&marks_dir(), &Timestamp::now().iso_date());
    let panicked = || (Err(NorthError::State("the pull panicked".into())), Err(NorthError::State("the swap panicked".into())));
    let pulled: Vec<(NorthResult<Snapshot>, NorthResult<Marks>)> = std::thread::scope(|scope| {
        let running: Vec<_> = far.iter().map(|(_, ssh)| scope.spawn(|| (OverSsh::default().api_spend(ssh), swapped(ssh, &held)))).collect();
        running.into_iter().map(|run| run.join().unwrap_or_else(|_| panicked())).collect()
    });
    for ((name, _), (pulled, marks)) in far.iter().zip(pulled) {
        failed.extend(adopted(name, marks));
        let stored = pulled.map_err(|error| error.user_message()).and_then(|snapshot| credit::store(&ledger(), &Snapshot { host: name.clone(), ..snapshot }));
        if let Err(error) = stored {
            failed.push(format!("api spend of {name}: {error}"));
        }
    }
    failed
}

fn swapped(ssh: &str, held: &Marks) -> NorthResult<Marks> {
    Ok(serde_json::from_slice(&OverSsh::default().run(ssh, &["fleet", "spool", "api-exhausted"], &serde_json::to_vec(held)?)?)?)
}

fn adopted(name: &str, sent: NorthResult<Marks>) -> Vec<String> {
    match sent.and_then(|sent| Ok(marks::adopt(&marks_dir(), &sent, &Timestamp::now().iso_date())?)) {
        Ok(ids) => ids.into_iter().map(|id| format!("api org {id} marked exhausted, as {name} marks it")).collect(),
        Err(error) => vec![format!("exhausted marks of {name}: {}", error.user_message())],
    }
}

/// Every `[hosts.NAME]` but this host's that declares an `ssh` or `deploy_ssh` destination, as (name, destination).
fn far_hosts(cfg: &toml_edit::DocumentMut, here: &str) -> Vec<(String, String)> {
    let hosts = cfg.get("hosts").and_then(toml_edit::Item::as_table_like);
    let declared = hosts.into_iter().flat_map(|hosts| hosts.iter());
    declared.filter(|(name, _)| *name != here).filter_map(|(name, host)| Some((name.to_owned(), gate::ssh_destination(["ssh", "deploy_ssh"].iter().find_map(|key| host.get(key)?.as_str())?)?.to_owned()))).collect()
}

#[cfg(test)]
mod tests;
