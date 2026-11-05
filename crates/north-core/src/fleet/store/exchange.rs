use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

use super::schema::sql;
use super::waits::{self, WaitKey, WaitRow};
use super::{FleetRow, GateRow, RowKind, SessionRow, Store};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Addressee {
    pub name: String,
    pub since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_since_ms: Option<u64>,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub taken: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub host: String,
    pub rows: Vec<FleetRow>,
    pub sessions: Vec<SessionRow>,
    pub addressees: Vec<Addressee>,
    #[serde(default)]
    pub gates: Vec<GateRow>,
    #[serde(default)]
    pub outranks: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Exchanged {
    pub held_elsewhere: Vec<String>,
    pub sessions: Vec<SessionRow>,
    pub rows: Vec<FleetRow>,
    #[serde(default)]
    pub gates: Vec<GateRow>,
    pub waits: Vec<WaitRow>,
    pub now: String,
}

pub trait LedgerHost {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged>;
    fn settle(&mut self, waits: &[WaitKey]) -> NorthResult<()>;
}

impl LedgerHost for Store {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged> {
        self.put(&exchange.rows)?;
        let held_elsewhere = self.publish_over(&exchange.host, &exchange.sessions, &exchange.outranks)?;
        self.publish_gates(&exchange.host, &exchange.gates)?;
        let names: Vec<&str> = exchange.addressees.iter().map(|addressee| addressee.name.as_str()).collect();
        let waits = waits::all(&self.db)?.into_iter().filter(|wait| names.contains(&wait.session.as_str())).collect();
        Ok(Exchanged { held_elsewhere, sessions: self.sessions()?, gates: self.gates()?, rows: addressed(&self.db, &exchange.addressees)?, waits, now: Timestamp::now().iso() })
    }

    fn settle(&mut self, waits: &[WaitKey]) -> NorthResult<()> {
        self.settle_all(waits)
    }
}

impl Store {
    pub fn addressed_to(&self, addressees: &[Addressee]) -> NorthResult<Vec<FleetRow>> {
        addressed(&self.db, addressees)
    }
}

const JOB_EXIT_KEPT_MS: u64 = 24 * 60 * 60 * 1000;

const ADDRESSED: &str = "SELECT events.data FROM events JOIN json_each(?1) AS addressee \
     ON json_extract(events.data, '$.to_node') = json_extract(addressee.value, '$.name') \
     WHERE events.kind IN (SELECT value FROM json_each(?2)) AND (events.kind = ?3 OR NOT COALESCE(json_extract(addressee.value, '$.stopped'), 0)) AND json_extract(events.data, '$.sent_at') >= CASE WHEN events.kind = ?3 THEN COALESCE(json_extract(addressee.value, '$.control_since_ms'), json_extract(addressee.value, '$.since_ms')) ELSE json_extract(addressee.value, '$.since_ms') END \
     AND (events.kind != ?4 OR events.ts >= ?5) \
     AND NOT EXISTS (SELECT 1 FROM json_each(addressee.value, '$.taken') AS taken WHERE taken.value = json_extract(events.data, '$.id')) \
     ORDER BY events.seq";

fn exit_floor() -> String {
    Timestamp::from_unix_millis(i64::try_from(super::super::runtime::now_ms().saturating_sub(JOB_EXIT_KEPT_MS)).unwrap_or(i64::MAX)).iso()
}

fn addressed(db: &Connection, addressees: &[Addressee]) -> NorthResult<Vec<FleetRow>> {
    let mut statement = db.prepare(ADDRESSED).map_err(sql)?;
    let data: Vec<String> = statement
        .query_map(rusqlite::params![serde_json::to_string(addressees)?, serde_json::to_string(&RowKind::DELIVERED)?, RowKind::Control.label(), RowKind::JobExit.label(), exit_floor()], |row| row.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    Ok(data.iter().filter_map(|data| readable(data)).collect())
}

fn readable(data: &str) -> Option<FleetRow> {
    serde_json::from_str(data).map_err(|error| eprintln!("north fleet: skipped a stored row that does not parse ({error}): {data}")).ok()
}
