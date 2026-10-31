use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{NorthError, NorthResult};

use super::row::RowKind;

const KIND_CHECK: &str = "CHECK(kind IN (";
const TABLE_HEADS: [&str; 2] = ["CREATE TABLE events(", "CREATE TABLE \"events\"("];

pub(super) fn sql(error: rusqlite::Error) -> NorthError {
    NorthError::Refused(format!("ledger: {error}"))
}

fn kind_list() -> String {
    RowKind::ALL.iter().map(|kind| format!("'{}'", kind.label())).collect::<Vec<_>>().join(",")
}

pub(super) fn ensure(db: &mut Connection) -> NorthResult<()> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    widen_events(&tx)?;
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS fleet_sessions(name TEXT PRIMARY KEY, host TEXT NOT NULL, status TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL) STRICT;
         CREATE UNIQUE INDEX IF NOT EXISTS events_fleet_id ON events(json_extract(data,'$.id')) WHERE kind IN ({});",
        kind_list()
    ))
    .map_err(sql)?;
    tx.commit().map_err(sql)
}

fn widen_events(tx: &Transaction) -> NorthResult<()> {
    let declared: String = tx
        .query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'events'", [], |row| row.get(0))
        .map_err(|_| NorthError::Refused("the ledger has no events table".into()))?;
    if RowKind::ALL.iter().all(|kind| declared.contains(&format!("'{}'", kind.label()))) {
        return Ok(());
    }
    let head = TABLE_HEADS
        .iter()
        .find(|head| declared.starts_with(**head))
        .filter(|_| declared.contains(KIND_CHECK))
        .ok_or_else(|| NorthError::Refused("the ledger's events table has an unexpected form; not widening it".into()))?;
    let widened = declared.replacen(head, "CREATE TABLE events_widened(", 1).replacen(KIND_CHECK, &format!("{KIND_CHECK}{},", kind_list()), 1);
    let indexes: Vec<String> = tx
        .prepare("SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = 'events' AND sql IS NOT NULL")
        .map_err(sql)?
        .query_map([], |row| row.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    tx.execute_batch(&widened).map_err(sql)?;
    tx.execute_batch(
        "INSERT INTO events_widened(seq,ts,item,kind,actor,tier,data) SELECT seq,ts,item,kind,actor,tier,data FROM events;
         DROP TABLE events;
         ALTER TABLE events_widened RENAME TO events;",
    )
    .map_err(sql)?;
    indexes.iter().try_for_each(|index| tx.execute_batch(index).map_err(sql))
}
