use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Refusal, State, Store};
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gate_lifecycle::{GateLife, GatePhase};
use crate::fleet::gates::{Answerer, Closure, Gate, GateBook, GateRequest};

use super::{Ledger, millis, sql};

const GATE_COLUMNS: &str = "gate_id,session,question,options,recommend,default_option,default_after_ms,opened_ms,answer,answered_by,closed_ms,verification,issue,box_text,ask_once";
const PHASE: &str = "(SELECT state FROM lifecycle_states WHERE machine = 'gate' AND instance = CAST(gate_id AS TEXT))";

fn instance(id: i64) -> String {
    id.to_string()
}

fn at(ms: u64) -> Timestamp {
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX))
}

fn settled_names() -> String {
    GatePhase::ALL.iter().filter(|phase| phase.is_settled()).map(|phase| format!("'{}'", phase.name())).collect::<Vec<_>>().join(",")
}

impl Ledger {
    /// Closes the gate seen in `seen` (`None` for a gate with no record yet), false when a gate already closed or another closer moved it first, which is not logged as an illegal move.
    pub(super) fn close_from(&self, id: i64, seen: Option<GatePhase>, closure: &Closure) -> NorthResult<bool> {
        if seen.is_some_and(|phase| !phase.is_open()) {
            return Ok(false);
        }
        let cause = format!("closed by {}", closure.by.label());
        let Ok(_) = Store::open(&self.path)?.transition_from::<GateLife>(&instance(id), seen, GatePhase::Delivering, &cause, at(closure.closed_ms))? else { return Ok(false) };
        self.write(|conn| conn.execute("UPDATE gates SET answer = ?2, answered_by = ?3, closed_ms = ?4 WHERE gate_id = ?1", params![id, closure.answer, closure.by.label(), millis(closure.closed_ms)]).map(drop))?;
        Ok(true)
    }

    fn move_gate_to(&self, id: i64, to: GatePhase, cause: &str, at_ms: u64) -> NorthResult<bool> {
        let moved = Store::open(&self.path)?.transition::<GateLife>(&instance(id), to, cause, at(at_ms))?;
        Ok(!matches!(moved, Err(Refusal::Held { .. } | Refusal::Illegal { .. })))
    }
}

impl GateBook for Ledger {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
        let options = serde_json::to_string(&request.options)?;
        let id = self.write(|conn| {
            conn.execute(
                "INSERT INTO gates(session, question, options, recommend, default_option, default_after_ms, opened_ms, verification, issue, box_text, ask_once) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![request.session, request.question, options, ordinal(request.recommend), ordinal(request.default), millis(request.default_after_ms), millis(opened_ms), request.verification, request.issue, request.box_text, request.ask_once],
            )?;
            Ok(conn.last_insert_rowid())
        })?;
        let phase = GatePhase::opened(request);
        let cause = if request.has_default() { "opened with a default to apply at its deadline" } else { "opened for Tom, who alone answers it" };
        Store::open(&self.path)?.transition_by::<GateLife>(&instance(id), phase, cause, at(opened_ms), phase.deadline(request, opened_ms))?.map_err(|refusal| NorthError::State(format!("gate {id} could not open: {refusal:?}")))?;
        Ok(id)
    }

    fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.with(|conn| conn.query_row(&format!("SELECT {GATE_COLUMNS}, {PHASE} FROM gates WHERE gate_id = ?1"), [id], gate_from_row).optional())
    }

    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        let settled = settled_names();
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {GATE_COLUMNS}, phase FROM (SELECT {GATE_COLUMNS}, {PHASE} AS phase FROM gates) \
                 WHERE phase IS NULL OR phase NOT IN ({settled}) OR closed_ms >= ?1 ORDER BY phase IN ({settled}), opened_ms + default_after_ms, gate_id"
            ))?;
            statement.query_map([millis(closed_since_ms)], gate_from_row)?.collect()
        })
    }

    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
        let seen = Store::open(&self.path)?.current::<GateLife>(&instance(id))?.map(|record| record.state);
        self.close_from(id, seen, closure)
    }

    fn move_gate(&self, id: i64, to: GatePhase, cause: &str, at_ms: u64) -> NorthResult<bool> {
        self.move_gate_to(id, to, cause, at_ms)
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
    let request = GateRequest {
        verification: row.get(11)?,
        issue: row.get(12)?,
        box_text: row.get(13)?,
        session: row.get(1)?,
        question: row.get(2)?,
        options: serde_json::from_str(&options).unwrap_or_default(),
        recommend: usize::try_from(unsigned(4)?).unwrap_or(0),
        default: usize::try_from(unsigned(5)?).unwrap_or(0),
        default_after_ms: unsigned(6)?,
        ask_once: row.get(14)?,
    };
    let phase = match row.get::<_, Option<String>>(15)? {
        Some(name) => GatePhase::parse(&name).ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(15, rusqlite::types::Type::Text, format!("gate {} is in {name}, a phase gates do not have", row.get::<_, i64>(0).unwrap_or(0)).into()))?,
        None => GatePhase::opened(&request),
    };
    Ok(Gate { id: row.get(0)?, request, opened_ms: unsigned(7)?, phase, closure })
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

/// Gives each gate of a ledger that predates the lifecycle record its phase, read once from the row: closed by an answerer, or open with or without a default.
pub(super) fn adopt(path: &std::path::Path, conn: &rusqlite::Connection) -> NorthResult<()> {
    let mut store = Store::open(path)?;
    let mut statement = conn.prepare(&format!("SELECT {GATE_COLUMNS} FROM gates ORDER BY gate_id")).map_err(sql)?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(10)?, row.get::<_, Option<String>>(9)?, row.get::<_, Option<String>>(8)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?))).map_err(sql)?;
    for row in rows {
        let (id, closed_ms, by, answer, default_option, default_after_ms, opened_ms) = row.map_err(sql)?;
        if store.current::<GateLife>(&instance(id))?.is_some() {
            continue;
        }
        let request = GateRequest { verification: false, issue: None, box_text: None, session: String::new(), question: String::new(), options: Vec::new(), recommend: 0, default: usize::try_from(default_option).unwrap_or(0), default_after_ms: u64::try_from(default_after_ms).unwrap_or(0), ask_once: None };
        let (phase, cause) = classify(closed_ms, by.as_deref(), answer.is_some(), &request).map_err(|why| NorthError::State(format!("gate {id} in the ledger: {why}")))?;
        let opened = u64::try_from(opened_ms).unwrap_or(0);
        let adopted = store.transition_by::<GateLife>(&instance(id), phase, &format!("adopted from the gates row: {cause}"), Timestamp::now(), phase.deadline(&request, opened))?;
        adopted.map_err(|refusal| NorthError::State(format!("gate {id} could not be adopted: {refusal:?}")))?;
    }
    Ok(())
}

/// The phase of a gate row from before the lifecycle record, or why its columns describe no phase.
fn classify(closed_ms: Option<i64>, by: Option<&str>, answered: bool, request: &GateRequest) -> Result<(GatePhase, &'static str), &'static str> {
    match (closed_ms, by.and_then(Answerer::parse), answered) {
        (None, None, false) if request.has_default() => Ok((GatePhase::CountingDown, "open with a default")),
        (None, None, false) => Ok((GatePhase::AskingTom, "open for Tom")),
        (Some(_), Some(by), true) => Ok((GatePhase::settled(&by), "closed with an answer")),
        (None, _, _) => Err("open yet carries an answer or an answerer"),
        (Some(_), _, _) => Err("closed without an answer or an answerer"),
    }
}
