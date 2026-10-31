use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

mod cache;
mod exchange;
mod remote;
mod registrar;
mod row;
mod schema;
mod sessions;
mod spool;
mod waits;

pub use cache::{cache_sessions, cached_sessions};
pub use exchange::{Addressee, Exchange, Exchanged, LedgerHost};
pub use remote::{FarRegistry, FarSpools, Handed, HandedRow, OverSsh, Pull, Undecodable, drop_with, set_aside_with, take_with};
pub use row::{AnswerPayload, ControlVerb, FleetRow, JobPayload, RowKind};
pub use registrar::{LedgerRegistrar, Registrar};
pub use sessions::{Claim, SessionRow};
pub use spool::{SPOOL_DIR, Spool, Spooled};
pub use waits::{JOB_LEASE_SECS, WaitKey, WaitKind, WaitRequest, WaitRow};

use schema::sql;

const BUSY: Duration = Duration::from_secs(60);
const LAUNCH_GRACE_MS: u64 = 10 * 60 * 1000;

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(ledger: &Path) -> NorthResult<Self> {
        let mut db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
        db.busy_timeout(BUSY).map_err(sql)?;
        schema::ensure(&mut db)?;
        waits::ensure(&db)?;
        Ok(Self { db })
    }

    pub fn put(&mut self, rows: &[FleetRow]) -> NorthResult<()> {
        rows.iter().try_for_each(FleetRow::validate)?;
        let now = Timestamp::now();
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        for row in rows {
            let inserted = tx.execute(
                "INSERT INTO events(ts, item, kind, actor, tier, data) VALUES (?1, NULL, ?2, ?3, '', ?4) ON CONFLICT DO NOTHING",
                rusqlite::params![now.iso(), row.kind.label(), row.from, serde_json::to_string(row)?],
            )
            .map_err(sql)?;
            if inserted == 1 {
                waits::record_job(&tx, row, now)?;
            }
        }
        tx.commit().map_err(sql)
    }

    pub fn register(&mut self, row: &SessionRow, claim: Claim) -> NorthResult<()> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        sessions::register(&tx, row, claim)?;
        tx.commit().map_err(sql)
    }

    pub fn publish(&mut self, host: &str, rows: &[SessionRow]) -> NorthResult<Vec<String>> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let held_elsewhere = sessions::publish(&tx, rows)?;
        let launching_since = Timestamp::from_unix_millis(i64::try_from(super::runtime::now_ms().saturating_sub(LAUNCH_GRACE_MS)).unwrap_or(i64::MAX)).iso();
        sessions::retire_vanished(&tx, host, rows, &launching_since)?;
        tx.commit().map_err(sql)?;
        Ok(held_elsewhere)
    }

    pub fn sessions(&self) -> NorthResult<Vec<SessionRow>> {
        sessions::all(&self.db)
    }

    pub fn wait(&mut self, request: &WaitRequest) -> NorthResult<()> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        waits::insert(&tx, request, Timestamp::now())?;
        tx.commit().map_err(sql)
    }

    pub fn settle(&mut self, session: &str, kind: WaitKind, key: &str) -> NorthResult<bool> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let settled = waits::settle(&tx, session, kind, key)?;
        tx.commit().map_err(sql)?;
        Ok(settled)
    }

    pub fn settle_all(&mut self, keys: &[WaitKey]) -> NorthResult<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        for key in keys {
            waits::settle(&tx, &key.session, key.kind, &key.key)?;
        }
        tx.commit().map_err(sql)
    }

    pub fn job_leased(&self, session: &str) -> NorthResult<bool> {
        waits::job_leased(&self.db, session, Timestamp::now())
    }

    pub fn waits(&self) -> NorthResult<Vec<WaitRow>> {
        waits::all(&self.db)
    }
}

#[cfg(test)]
mod tests;
