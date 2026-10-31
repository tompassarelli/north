use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;

use super::schema::sql;
use super::{FleetRow, RowKind, SessionRow, Store};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Addressee {
    pub name: String,
    pub since_ms: u64,
    #[serde(default)]
    pub taken: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub rows: Vec<FleetRow>,
    pub sessions: Vec<SessionRow>,
    pub addressees: Vec<Addressee>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Exchanged {
    pub held_elsewhere: Vec<String>,
    pub sessions: Vec<SessionRow>,
    pub rows: Vec<FleetRow>,
}

pub trait LedgerHost {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged>;
}

impl LedgerHost for Store {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged> {
        self.put(&exchange.rows)?;
        let held_elsewhere = self.publish(&exchange.sessions)?;
        Ok(Exchanged { held_elsewhere, sessions: self.sessions()?, rows: addressed(&self.db, &exchange.addressees)? })
    }
}

const ADDRESSED: &str = "SELECT events.data FROM events JOIN json_each(?1) AS addressee \
     ON json_extract(events.data, '$.to_node') = json_extract(addressee.value, '$.name') \
     WHERE events.kind = ?2 AND json_extract(events.data, '$.sent_at') >= json_extract(addressee.value, '$.since_ms') \
     AND NOT EXISTS (SELECT 1 FROM json_each(addressee.value, '$.taken') AS taken WHERE taken.value = json_extract(events.data, '$.id')) \
     ORDER BY events.seq";

fn addressed(db: &Connection, addressees: &[Addressee]) -> NorthResult<Vec<FleetRow>> {
    let mut statement = db.prepare(ADDRESSED).map_err(sql)?;
    let data: Vec<String> = statement
        .query_map(rusqlite::params![serde_json::to_string(addressees)?, RowKind::Message.label()], |row| row.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    data.iter().map(|data| Ok(serde_json::from_str(data)?)).collect()
}
