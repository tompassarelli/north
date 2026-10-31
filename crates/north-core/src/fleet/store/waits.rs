use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::row::{FleetRow, RowKind};
use super::schema::sql;

pub const JOB_LEASE_SECS: u64 = 180;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaitKind {
    Job,
    Farm,
    Branch,
    Capture,
}

impl WaitKind {
    pub const ALL: [WaitKind; 4] = [Self::Job, Self::Farm, Self::Branch, Self::Capture];

    pub fn label(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Farm => "farm",
            Self::Branch => "branch",
            Self::Capture => "capture",
        }
    }

    fn parse(label: &str) -> NorthResult<Self> {
        Self::ALL.into_iter().find(|kind| kind.label() == label).ok_or_else(|| NorthError::Refused(format!("ledger: unknown wait kind {label}")))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WaitRow {
    pub session: String,
    pub kind: WaitKind,
    pub key: String,
    pub deadline: String,
    pub created: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WaitKey {
    pub session: String,
    pub kind: WaitKind,
    pub key: String,
}

impl WaitRow {
    pub fn key(&self) -> WaitKey {
        WaitKey { session: self.session.clone(), kind: self.kind, key: self.key.clone() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WaitRequest {
    pub session: String,
    pub kind: WaitKind,
    pub key: String,
    pub within_secs: u64,
}

impl WaitRequest {
    pub fn key(&self) -> WaitKey {
        WaitKey { session: self.session.clone(), kind: self.kind, key: self.key.clone() }
    }

    pub fn parse(input: &[u8]) -> NorthResult<Self> {
        let request: Self = serde_json::from_slice(input)?;
        if request.session.trim().is_empty() || request.key.trim().is_empty() {
            return Err(NorthError::Usage("a wait needs a session and a key".into()));
        }
        Ok(request)
    }
}

pub(super) fn ensure(db: &Connection) -> NorthResult<()> {
    let kinds = WaitKind::ALL.iter().map(|kind| format!("'{}'", kind.label())).collect::<Vec<_>>().join(",");
    db.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS fleet_waits(session TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ({kinds})), key TEXT NOT NULL, deadline TEXT NOT NULL, created TEXT NOT NULL, PRIMARY KEY(session, kind, key)) STRICT;"
    ))
    .map_err(sql)
}

fn deadline_after(now: Timestamp, secs: u64) -> String {
    Timestamp::from_unix_seconds(now.unix_seconds().saturating_add(i64::try_from(secs).unwrap_or(i64::MAX))).iso()
}

pub(super) fn insert(tx: &Transaction, request: &WaitRequest, now: Timestamp) -> NorthResult<()> {
    tx.execute(
        "INSERT INTO fleet_waits(session, kind, key, deadline, created) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(session, kind, key) DO UPDATE SET deadline = excluded.deadline",
        params![request.session, request.kind.label(), request.key, deadline_after(now, request.within_secs), now.iso()],
    )
    .map_err(sql)?;
    Ok(())
}

pub(super) fn settle(tx: &Transaction, session: &str, kind: WaitKind, key: &str) -> NorthResult<bool> {
    Ok(tx.execute("DELETE FROM fleet_waits WHERE session = ?1 AND kind = ?2 AND key = ?3", params![session, kind.label(), key]).map_err(sql)? == 1)
}

pub(super) fn record_job(tx: &Transaction, row: &FleetRow, now: Timestamp) -> NorthResult<()> {
    let Some(job) = &row.job else { return Ok(()) };
    let lease = job.lease_secs.unwrap_or(JOB_LEASE_SECS);
    match row.kind {
        RowKind::JobStart => insert(tx, &WaitRequest { session: row.to.clone(), kind: WaitKind::Job, key: job.key.clone(), within_secs: lease }, now),
        RowKind::JobBeat => tx
            .execute("UPDATE fleet_waits SET deadline = MAX(deadline, ?3) WHERE session = ?1 AND kind = 'job' AND key = ?2", params![row.to, job.key, deadline_after(now, lease)])
            .map(drop)
            .map_err(sql),
        RowKind::JobExit => settle(tx, &row.to, WaitKind::Job, &job.key).map(drop),
        _ => Ok(()),
    }
}

pub(super) fn all(db: &Connection) -> NorthResult<Vec<WaitRow>> {
    let mut statement = db.prepare("SELECT session, kind, key, deadline, created FROM fleet_waits ORDER BY session, kind, key").map_err(sql)?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    rows.into_iter().map(|(session, kind, key, deadline, created)| Ok(WaitRow { session, kind: WaitKind::parse(&kind)?, key, deadline, created })).collect()
}

pub(super) fn job_leased(db: &Connection, session: &str, now: Timestamp) -> NorthResult<bool> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM fleet_waits WHERE session = ?1 AND kind = 'job' AND deadline > ?2)", params![session, now.iso()], |row| row.get(0)).map_err(sql)
}
