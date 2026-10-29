use rusqlite::{OptionalExtension, Row, params};

use crate::error::NorthResult;
use crate::fleet::gates::{Answerer, Closure, Gate, GateBook, GateRequest};

use super::{Ledger, millis};

const GATE_COLUMNS: &str = "gate_id,session,question,options,recommend,default_option,default_after_ms,opened_ms,answer,answered_by,closed_ms,verification";

impl GateBook for Ledger {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
        let options = serde_json::to_string(&request.options)?;
        self.write(|conn| {
            conn.execute(
                "INSERT INTO gates(session, question, options, recommend, default_option, default_after_ms, opened_ms, verification) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![request.session, request.question, options, ordinal(request.recommend), ordinal(request.default), millis(request.default_after_ms), millis(opened_ms), request.verification],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.with(|conn| conn.query_row(&format!("SELECT {GATE_COLUMNS} FROM gates WHERE gate_id = ?1"), [id], gate_from_row).optional())
    }

    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {GATE_COLUMNS} FROM gates WHERE closed_ms IS NULL OR closed_ms >= ?1 ORDER BY closed_ms IS NOT NULL, opened_ms + default_after_ms, gate_id"
            ))?;
            statement.query_map([millis(closed_since_ms)], gate_from_row)?.collect()
        })
    }

    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
        self.write(|conn| {
            conn.execute(
                "UPDATE gates SET answer = ?2, answered_by = ?3, closed_ms = ?4 WHERE gate_id = ?1 AND closed_ms IS NULL",
                params![id, closure.answer, closure.by.label(), millis(closure.closed_ms)],
            )
            .map(|changed| changed == 1)
        })
    }
}

fn gate_from_row(row: &Row<'_>) -> rusqlite::Result<Gate> {
    let options: String = row.get(3)?;
    let unsigned = |index: usize| row.get::<_, i64>(index).map(|value| u64::try_from(value).unwrap_or(0));
    let answer: Option<String> = row.get(8)?;
    let by: Option<String> = row.get(9)?;
    let closed_ms: Option<i64> = row.get(10)?;
    let closure = match (answer, by.as_deref().and_then(Answerer::parse), closed_ms) {
        (Some(answer), Some(by), Some(closed_ms)) => Some(Closure { answer, by, closed_ms: u64::try_from(closed_ms).unwrap_or(0) }),
        _ => None,
    };
    Ok(Gate {
        id: row.get(0)?,
        request: GateRequest {
            verification: row.get(11)?,
            session: row.get(1)?,
            question: row.get(2)?,
            options: serde_json::from_str(&options).unwrap_or_default(),
            recommend: usize::try_from(unsigned(4)?).unwrap_or(0),
            default: usize::try_from(unsigned(5)?).unwrap_or(0),
            default_after_ms: unsigned(6)?,
        },
        opened_ms: unsigned(7)?,
        closure,
    })
}

fn ordinal(number: usize) -> i64 {
    i64::try_from(number).unwrap_or(i64::MAX)
}

pub(super) fn allow_session_answerers(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let schema: String = conn.query_row("SELECT sql FROM sqlite_master WHERE name = 'gates'", [], |row| row.get(0))?;
    if schema.contains("CHECK(answered_by") {
        conn.execute_batch("ALTER TABLE gates RENAME TO gates_restricted;")?;
        conn.execute_batch(super::SCHEMA)?;
        conn.execute_batch("INSERT INTO gates SELECT * FROM gates_restricted; DROP TABLE gates_restricted;")?;
    }
    Ok(())
}
