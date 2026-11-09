use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use crate::error::NorthResult;
use crate::sender::Sender;

use super::BUSY;
use super::row::RowKind;
use super::schema::sql;

pub fn delivered_by(ledger: &Path, sender: &Sender) -> NorthResult<HashSet<(String, String)>> {
    if !ledger.exists() {
        return Ok(HashSet::new());
    }
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
    db.busy_timeout(BUSY).map_err(sql)?;
    let mut statement = db
        .prepare(
            "SELECT s.conversation, json_extract(e.data, '$.text') FROM events e JOIN fleet_sessions s ON s.name = json_extract(e.data, '$.to_node') \
             WHERE e.kind = ?1 AND json_extract(e.data, '$.sender') = ?2 AND s.conversation IS NOT NULL",
        )
        .map_err(sql)?;
    let rows = statement.query_map(params![RowKind::Message.label(), sender.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))).map_err(sql)?;
    rows.filter_map(|row| row.map(|(conversation, text)| text.map(|text| (conversation, text))).transpose()).collect::<Result<_, _>>().map_err(sql)
}
