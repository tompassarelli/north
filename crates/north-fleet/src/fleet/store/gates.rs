use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::fleet::gates::{Gate, GateRequest};

use super::schema::sql;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GateRow {
    pub host: String,
    pub id: i64,
    pub request: GateRequest,
    pub opened_ms: u64,
}

impl GateRow {
    pub fn open(host: &str, gate: &Gate) -> Self {
        Self { host: host.to_owned(), id: gate.id, request: gate.request.clone(), opened_ms: gate.opened_ms }
    }

    pub fn label(&self) -> String {
        format!("{}:{}", self.host, self.id)
    }

    pub fn default_at_ms(&self) -> u64 {
        self.opened_ms.saturating_add(self.request.default_after_ms)
    }
}

pub(super) fn ensure(db: &Connection) -> NorthResult<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS fleet_gates(host TEXT NOT NULL, id INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)), PRIMARY KEY(host, id)) STRICT;").map_err(sql)
}

pub(super) fn publish(tx: &Transaction, host: &str, rows: &[GateRow]) -> NorthResult<()> {
    tx.execute("DELETE FROM fleet_gates WHERE host = ?1", [host]).map_err(sql)?;
    for row in rows.iter().filter(|row| row.host == host) {
        tx.execute("INSERT INTO fleet_gates(host, id, data) VALUES (?1, ?2, ?3)", params![row.host, row.id, serde_json::to_string(row)?]).map_err(sql)?;
    }
    Ok(())
}

pub(super) fn all(db: &Connection) -> NorthResult<Vec<GateRow>> {
    let mut statement = db.prepare("SELECT data FROM fleet_gates ORDER BY host, id").map_err(sql)?;
    let data: Vec<String> = statement.query_map([], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    data.iter().map(|data| Ok(serde_json::from_str(data)?)).collect()
}
