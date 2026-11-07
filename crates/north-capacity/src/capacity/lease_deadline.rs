use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use serde_json::Value;

use super::lease_lifecycle::{Ledger, Phase};

const LOCK_PATIENCE: Duration = Duration::from_secs(2);

pub fn pause(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |ledger, lease| {
        let id = lease["id"].as_str().unwrap_or_default();
        if ledger.adopt(lease).ok().flatten() == Some(Phase::Frozen) {
            return false;
        }
        let expires_at = lease["expiresAt"].as_f64();
        let _ = ledger.advance(id, Phase::Frozen, "a gaming or pressure hold froze its scope", expires_at);
        false
    });
}

pub fn resume(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |ledger, lease| {
        let id = lease["id"].as_str().unwrap_or_default();
        let Some(frozen) = ledger.record(id).ok().flatten().filter(|record| record.state == Phase::Frozen) else {
            return false;
        };
        let paused = u64::try_from(Timestamp::now().unix_millis() - frozen.since.unix_millis()).unwrap_or(0);
        let expires_at = lease["expiresAt"].as_u64().map(|expires_at| expires_at + paused);
        let back = if lease["kind"] == "run" { Phase::Running } else { Phase::Reserved };
        if ledger.advance(id, back, "the hold ended and its scope thawed", expires_at.map(|at| at as f64)).ok().flatten().is_none() {
            return false;
        }
        if let Some(fields) = lease.as_object_mut() {
            fields.remove("frozenAt");
        }
        if let Some(expires_at) = expires_at {
            lease["expiresAt"] = expires_at.into();
        }
        true
    });
}

pub fn reconcile(capacity: &Path, frozen: &BTreeSet<String>, thawed: &BTreeSet<String>) {
    let Ok(ledger) = Ledger::open(capacity) else { return };
    let phase = |id: &String| ledger.phase(id).ok().flatten();
    let to_freeze = frozen.iter().filter(|id| phase(id) != Some(Phase::Frozen)).cloned().collect::<BTreeSet<_>>();
    let to_thaw = thawed.iter().filter(|id| phase(id) == Some(Phase::Frozen)).cloned().collect::<BTreeSet<_>>();
    drop(ledger);
    pause(capacity, &to_freeze);
    resume(capacity, &to_thaw);
}

fn rewrite(capacity: &Path, ids: &BTreeSet<String>, change: impl Fn(&mut Ledger, &mut Value) -> bool) {
    if ids.is_empty() {
        return;
    }
    let Some(_lock) = lock(capacity) else { return };
    let Ok(mut ledger) = Ledger::open(capacity) else { return };
    for path in ids.iter().map(|id| capacity.join("leases").join(format!("{id}.json"))) {
        let Some(mut lease) = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { continue };
        if change(&mut ledger, &mut lease) {
            let _ = replace(&path, &format!("{lease}\n"));
        }
    }
}

fn replace(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temporary)?.write_all(text.as_bytes())?;
    fs::rename(temporary, path)
}

fn lock(capacity: &Path) -> Option<rusqlite::Connection> {
    let connection = rusqlite::Connection::open(capacity.join("admission.lock")).ok()?;
    connection.busy_timeout(LOCK_PATIENCE).ok()?;
    connection.execute_batch("BEGIN EXCLUSIVE").ok()?;
    Some(connection)
}
