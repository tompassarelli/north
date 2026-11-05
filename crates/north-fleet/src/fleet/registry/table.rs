use crate::error::NorthResult;
use crate::fleet::gates::{self, Gate};
use crate::fleet::model::SessionRecord;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{FleetTable, GateRow, SessionRow, cache_sessions, cache_table, cached_gates, cached_sessions};
use crate::foundation::clock::Timestamp;

use super::Fleet;

const STALE_AFTER_DEFAULT_MS: u64 = 24 * 60 * 60 * 1000;

impl Fleet {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn cache_sessions(&self, rows: &[SessionRow]) -> NorthResult<()> {
        cache_sessions(&self.root, rows)
    }

    pub fn cache_table(&self, table: &FleetTable) -> NorthResult<()> {
        cache_table(&self.root, table)
    }

    pub fn open_gate_rows(&self) -> NorthResult<Vec<GateRow>> {
        Ok(gates::decisions(self.ledger.gates(u64::MAX)?).iter().filter(|gate| Gate::is_open(gate)).map(|gate| GateRow::open(&self.host, gate)).collect())
    }

    pub fn gates_elsewhere(&self) -> NorthResult<Vec<GateRow>> {
        let now = now_ms();
        Ok(cached_gates(&self.root)?.into_iter().filter(|gate| gate.host != self.host && now < gate.default_at_ms().saturating_add(STALE_AFTER_DEFAULT_MS)).collect())
    }

    pub fn cached_sessions(&self) -> NorthResult<Option<Vec<SessionRow>>> {
        cached_sessions(&self.root)
    }

    pub(super) fn uncache_session(&self, name: &str) -> NorthResult<()> {
        match cached_sessions(&self.root)? {
            Some(rows) if rows.iter().any(|row| row.name == name) => cache_sessions(&self.root, &rows.into_iter().filter(|row| row.name != name).collect::<Vec<_>>()),
            _ => Ok(()),
        }
    }

    pub fn table_row(&self, record: &SessionRecord, now_ms: u64) -> SessionRow {
        SessionRow {
            name: record.name.clone(),
            host: self.host.clone(),
            status: self.status(record).label().to_owned(),
            created: iso(record.created_ms),
            updated: iso(now_ms),
            provider: record.provider.label().to_owned(),
            account: record.account.clone(),
            cwd: record.cwd.display().to_string(),
            parent: record.parent.clone(),
            placement: record.placement.clone(),
            machine: record.machine.as_ref().map(|machine| machine.name.clone()),
            tasks: record.tasks().map(str::to_owned).collect(),
        }
    }
}

fn iso(ms: u64) -> String {
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX)).iso()
}
