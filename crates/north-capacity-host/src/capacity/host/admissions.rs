//! Each admission's outcome, kept in capacity.db until the worker ledger ships it to the threads ledger as a `capacity` row.
use serde_json::Value;

use super::database::{Admission, admission_rows, capacity_database, capacity_database_readonly, insert_admission};
use super::json::{Obj, field, text};
use super::request::Request;

/// Records the outcome that ended `request`'s admission: the lease it reserved, or the deferral or refusal it was left with.
pub(super) fn record(owner: &str, request: &Request, admitted: &Obj) {
    let view = admitted.value();
    let lease = text(&view, "lease").map(str::to_owned);
    let id = lease.unwrap_or_else(super::machine::uuid);
    let scope = view.get("scope").unwrap_or(&Value::Null);
    let row = Admission {
        id: &id,
        owner,
        class: &request.name,
        decision: text(&view, "decision").unwrap_or_default(),
        reason: text(&view, "reason").unwrap_or_default(),
        queued_seconds: field(&view, "queuedSeconds").unwrap_or(0.0),
        cpus: field(scope, "cpus"),
        memory_mib: field(scope, "memoryMiB"),
    };
    if let Err(error) = capacity_database().and_then(|db| insert_admission(&db, &row)) {
        eprintln!("machine-capacity: capacity database failed: {error}");
    }
}

/// This host's admissions from `since` on, as the ledger's `capacity` rows.
pub fn ledger_rows(host: &str, since: Option<&str>) -> Vec<Value> {
    capacity_database_readonly().and_then(|db| admission_rows(&db, host, since).ok()).unwrap_or_default()
}
