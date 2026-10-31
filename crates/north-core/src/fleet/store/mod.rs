use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

mod row;
mod schema;
mod sessions;

pub use row::{FleetRow, JobPayload, RowKind};
pub use sessions::SessionRow;

use schema::sql;

const BUSY: Duration = Duration::from_secs(60);

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(ledger: &Path) -> NorthResult<Self> {
        let mut db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
        db.busy_timeout(BUSY).map_err(sql)?;
        schema::ensure(&mut db)?;
        Ok(Self { db })
    }

    pub fn put(&mut self, rows: &[FleetRow]) -> NorthResult<()> {
        rows.iter().try_for_each(FleetRow::validate)?;
        let now = Timestamp::now().iso();
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        for row in rows {
            tx.execute(
                "INSERT INTO events(ts, item, kind, actor, tier, data) VALUES (?1, NULL, ?2, ?3, '', ?4) ON CONFLICT DO NOTHING",
                rusqlite::params![now, row.kind.label(), row.from, serde_json::to_string(row)?],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    pub fn register(&mut self, row: &SessionRow) -> NorthResult<()> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        sessions::register(&tx, row)?;
        tx.commit().map_err(sql)
    }

    pub fn publish(&mut self, rows: &[SessionRow]) -> NorthResult<Vec<String>> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let held_elsewhere = sessions::publish(&tx, rows)?;
        tx.commit().map_err(sql)?;
        Ok(held_elsewhere)
    }

    pub fn sessions(&self) -> NorthResult<Vec<SessionRow>> {
        sessions::all(&self.db)
    }
}

#[cfg(test)]
mod tests;
