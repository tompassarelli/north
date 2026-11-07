use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::NorthResult;

use super::{GateRow, SessionRow, WaitKind, WaitRow};

const SESSIONS: &str = ".fleet-sessions.json";
const GATES: &str = ".fleet-gates.json";
const LEASES: &str = ".fleet-leases.json";
const LEASES_FRESH_MS: u64 = 60_000;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FleetTable {
    pub sessions: Vec<SessionRow>,
    pub gates: Vec<GateRow>,
}

pub fn cache_table(root: &Path, table: &FleetTable) -> NorthResult<()> {
    cache_sessions(root, &table.sessions)?;
    write(root, GATES, &table.gates)
}

pub fn cache_sessions(root: &Path, rows: &[SessionRow]) -> NorthResult<()> {
    write(root, SESSIONS, rows)
}

pub fn cached_sessions(root: &Path) -> NorthResult<Option<Vec<SessionRow>>> {
    read(root, SESSIONS)
}

pub fn cached_gates(root: &Path) -> NorthResult<Vec<GateRow>> {
    Ok(read(root, GATES)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LeaseTable {
    pub read_ms: u64,
    pub covered: BTreeSet<String>,
    pub held: BTreeSet<String>,
}

impl LeaseTable {
    pub fn read(waits: &[WaitRow], covered: impl IntoIterator<Item = String>, ledger_now: &str, read_ms: u64) -> Self {
        let held = waits.iter().filter(|wait| wait.kind == WaitKind::Job && wait.deadline.as_str() > ledger_now).map(|wait| wait.session.clone()).collect();
        Self { read_ms, covered: covered.into_iter().collect(), held }
    }

    pub fn leased(&self, session: &str, now_ms: u64) -> Option<bool> {
        (now_ms.saturating_sub(self.read_ms) <= LEASES_FRESH_MS && self.covered.contains(session)).then(|| self.held.contains(session))
    }
}

pub fn cache_leases(root: &Path, table: &LeaseTable) -> NorthResult<()> {
    write(root, LEASES, table)
}

pub fn cached_leases(root: &Path) -> NorthResult<Option<LeaseTable>> {
    read(root, LEASES)
}

fn write<T: Serialize + ?Sized>(root: &Path, name: &str, rows: &T) -> NorthResult<()> {
    let staged = root.join(format!("{name}.{}.{}.tmp", std::process::id(), super::super::runtime::now_ms()));
    fs::write(&staged, serde_json::to_vec(rows)?)?;
    fs::rename(staged, root.join(name))?;
    Ok(())
}

fn read<T: for<'de> Deserialize<'de>>(root: &Path, name: &str) -> NorthResult<Option<T>> {
    match fs::read(root.join(name)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
