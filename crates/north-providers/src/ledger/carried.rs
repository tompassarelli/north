use std::path::{Path, PathBuf};

use super::CheckRun;

/// Queues a Done-when verdict for the ledger host's threads database, where its graph reads it, when `ledger` is this host's own; a scratch ledger carries nothing.
pub(super) fn carry(ledger: &Path, check: &CheckRun) {
    let Some(sha) = check.head_sha.filter(|sha| !sha.is_empty()) else { return };
    if default_ledger().as_deref() != Some(ledger) {
        return;
    }
    let row = serde_json::json!({
        "type": crate::threads::done_checks::KIND,
        "host": crate::platform::host::name(),
        "task_ref": check.task_ref,
        "head_sha": sha,
        "command": check.command,
        "exit": check.exit,
        "ran_ms": check.ran_ms,
    });
    if let Err(why) = crate::threads::client::spool_ingest(format!("{row}\n").as_bytes()) {
        eprintln!("north ledger: the check of {} at {sha} is not queued for the ledger host: {why}", check.task_ref);
    }
}

pub(super) fn default_ledger() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|value| !value.is_empty()).map(|home| PathBuf::from(home).join(".local/state/north/ledger.db"))
}
