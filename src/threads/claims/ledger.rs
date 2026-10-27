//! Claims as the NorthThreads SQLite ledger stores them; bin/threads owns the schema.
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

use super::{Claim, Issue, Standing};
use crate::foundation::clock::Timestamp;

const BUSY: Duration = Duration::from_secs(60);
const CLAIMS: &str = "SELECT c.item, c.owner, c.since, c.eta_min, \
    (SELECT max(e.ts) FROM events e WHERE e.item = c.item AND e.actor = c.owner) FROM claims c";

/// What `take` did with an item's claim row.
#[derive(Debug, PartialEq)]
pub(crate) enum Taken {
    Fresh,
    Renewed,
    Voided { previous: Claim, why: &'static str },
    Takeover { previous: Claim, idle_since: i64 },
    Refused(Claim),
}

pub(crate) fn open(path: &Path, write: bool) -> Result<Connection, String> {
    let access = if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let failed = |error: rusqlite::Error| format!("ledger {}: {error}", path.display());
    let db = Connection::open_with_flags(path, access | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    db.busy_timeout(BUSY).map_err(failed)?;
    Ok(db)
}

fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse(text).ok().map(Timestamp::unix_seconds)
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Claim> {
    let since: String = row.get(2)?;
    let seen: Option<String> = row.get(4)?;
    Ok(Claim {
        item: row.get(0)?,
        owner: row.get(1)?,
        since: seconds(&since).unwrap_or(0),
        eta_min: row.get(3)?,
        owner_seen: seen.as_deref().and_then(seconds),
    })
}

/// Every claim row, oldest first.
pub(crate) fn claims(db: &Connection) -> Result<Vec<Claim>, String> {
    let mut statement = db.prepare(&format!("{CLAIMS} ORDER BY c.since")).map_err(|error| error.to_string())?;
    let rows = statement.query_map([], row).map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|error| error.to_string())
}

/// A native item's standing from the ledger's own item table.
pub(crate) fn native(db: &Connection, ns: &str, number: u64) -> Issue {
    let Ok(number) = i64::try_from(number) else { return Issue::Missing };
    let found = db
        .query_row("SELECT created, state FROM items WHERE ns = ?1 AND number = ?2", params![ns, number], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .optional();
    match found {
        Ok(None) => Issue::Missing,
        Ok(Some((_, state))) if state != "open" => Issue::Closed,
        Ok(Some((created, _))) => seconds(&created).map_or(Issue::Unread, |opened| Issue::Open { opened }),
        Err(_) => Issue::Unread,
    }
}

fn event(db: &Connection, at: &str, item: &str, kind: &str, actor: &str, data: &serde_json::Value) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO events(ts, item, kind, actor, data) VALUES (?1, ?2, ?3, ?4, ?5)", params![at, item, kind, actor, data.to_string()])
}

/// Claims `item` for `by` in one immediate transaction: a new claim, a renewal of `by`'s own, a
/// replacement of a void claim, a takeover of a stale one, or a refusal by a live one.
pub(crate) fn take(db: &mut Connection, item: &str, by: &str, eta_min: Option<i64>, issue: Issue, now: i64) -> Result<Taken, String> {
    let at = Timestamp::from_unix_seconds(now).iso();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    let held = tx.query_row(&format!("{CLAIMS} WHERE c.item = ?1"), [item], row).optional().map_err(|error| error.to_string())?;
    let taken = match held {
        None => Taken::Fresh,
        Some(claim) if claim.owner == by => Taken::Renewed,
        Some(claim) => match claim.standing(issue, now) {
            Standing::Live => Taken::Refused(claim),
            Standing::Void(why) => Taken::Voided { previous: claim, why },
            Standing::Stale { idle_since } => Taken::Takeover { previous: claim, idle_since },
        },
    };
    let written = match &taken {
        Taken::Refused(_) => Ok(0),
        Taken::Fresh => tx
            .execute("INSERT INTO claims(item, owner, since, eta_min) VALUES (?1, ?2, ?3, ?4)", params![item, by, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min}))),
        Taken::Renewed => tx
            .execute("UPDATE claims SET since = ?2, eta_min = COALESCE(?3, eta_min) WHERE item = ?1", params![item, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min}))),
        Taken::Voided { previous, why } => tx
            .execute("UPDATE claims SET owner = ?2, since = ?3, eta_min = ?4 WHERE item = ?1", params![item, by, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min, "void": previous.owner, "why": why}))),
        Taken::Takeover { previous, idle_since } => {
            let idle = Timestamp::from_unix_seconds(*idle_since).iso();
            tx.execute("UPDATE claims SET owner = ?2, since = ?3, eta_min = ?4 WHERE item = ?1", params![item, by, at, eta_min]).and_then(|_| {
                event(&tx, &at, item, "handoff", by, &json!({"from": previous.owner, "to": by, "takeover": format!("stale: no owner event since {idle}")}))
            })
        }
    };
    written.map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(taken)
}
