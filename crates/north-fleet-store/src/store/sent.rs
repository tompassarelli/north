use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use crate::error::NorthResult;
use crate::sender::Sender;

use super::BUSY;
use super::row::RowKind;
use super::schema::sql;

pub fn texts_sent_by(ledger: &Path, sender: &Sender) -> NorthResult<HashSet<String>> {
    if !ledger.exists() {
        return Ok(HashSet::new());
    }
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
    db.busy_timeout(BUSY).map_err(sql)?;
    let mut statement = db.prepare("SELECT json_extract(data, '$.text') FROM events WHERE kind = ?1 AND json_extract(data, '$.sender') = ?2").map_err(sql)?;
    let texts = statement.query_map(params![RowKind::Message.label(), sender.to_string()], |row| row.get::<_, Option<String>>(0)).map_err(sql)?;
    texts.filter_map(Result::transpose).collect::<Result<_, _>>().map_err(sql)
}
