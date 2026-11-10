//! Claims as the NorthThreads SQLite ledger stores them; threads::schema creates the table.
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

use super::{Claim, Issue, Standing, key, split_key};
use crate::foundation::clock::Timestamp;

const BUSY: Duration = Duration::from_secs(60);
const CLAIMS: &str = "SELECT c.item, c.owner, c.since, c.eta_min, \
    (SELECT max(e.ts) FROM events e WHERE e.item = CASE WHEN instr(c.item, '@') > 0 THEN substr(c.item, 1, instr(c.item, '@') - 1) ELSE c.item END AND e.actor = c.owner) FROM claims c";

/// Who takes a claim, and the sessions above them in the fleet tree.
pub struct Claimant<'a> {
    pub name: &'a str,
    pub ancestors: &'a BTreeSet<String>,
}

/// What `take` did with an item's claim row.
#[derive(Debug, PartialEq)]
pub enum Taken {
    Fresh,
    Renewed,
    Voided { previous: Claim, why: &'static str },
    Takeover { previous: Claim, idle_since: i64 },
    Refused(Claim),
}

pub fn open(path: &Path, write: bool) -> Result<Connection, String> {
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
    let (item, slug) = split_key(&row.get::<_, String>(0)?);
    Ok(Claim {
        item,
        slug,
        owner: row.get(1)?,
        since: seconds(&since).unwrap_or(0),
        eta_min: row.get(3)?,
        owner_seen: seen.as_deref().and_then(seconds),
    })
}

/// Every claim row, oldest first.
pub fn claims(db: &Connection) -> Result<Vec<Claim>, String> {
    let mut statement = db.prepare(&format!("{CLAIMS} ORDER BY c.since")).map_err(|error| error.to_string())?;
    let rows = statement.query_map([], row).map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|error| error.to_string())
}

/// The (claims key, owner) rows on `items`, whole or by box, and those `owners` hold, one lookup per item on the claims key or per owner, each row once.
pub fn holders(db: &Connection, items: &[String], owners: &[String]) -> Result<Vec<(String, String)>, String> {
    let failed = |error: rusqlite::Error| error.to_string();
    let pair = |row: &rusqlite::Row<'_>| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
    let mut found = BTreeSet::new();
    let mut by_item = db.prepare("SELECT item, owner FROM claims WHERE item = ?1 OR substr(item, 1, length(?1) + 1) = ?1 || '@'").map_err(failed)?;
    for item in items {
        for row in by_item.query_map([item], pair).map_err(failed)? {
            found.insert(row.map_err(failed)?);
        }
    }
    let mut by_owner = db.prepare("SELECT item, owner FROM claims WHERE owner = ?1").map_err(failed)?;
    for owner in owners {
        for row in by_owner.query_map([owner], pair).map_err(failed)? {
            found.insert(row.map_err(failed)?);
        }
    }
    Ok(found.into_iter().collect())
}
/// A native item's standing from the ledger's own item table.
pub fn native(db: &Connection, ns: &str, number: u64) -> Issue {
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

fn decide((held, beside): (Option<&Claim>, Option<&Claim>), by: &str, (issue, alive, now): (Issue, Option<&BTreeSet<String>>, i64)) -> Taken {
    match (held, beside) {
        (_, Some(claim)) => Taken::Refused(claim.clone()),
        (None, None) => Taken::Fresh,
        (Some(claim), None) if claim.owner == by => Taken::Renewed,
        (Some(claim), None) => match claim.standing(issue, alive, now) {
            Standing::Live => Taken::Refused(claim.clone()),
            Standing::Void(why) => Taken::Voided { previous: claim.clone(), why },
            Standing::Stale { idle_since } => Taken::Takeover { previous: claim.clone(), idle_since },
        },
    }
}

/// Whether `claim` is a live claim beside the one `by` is taking for `target`: a whole-issue claim blocks a box claim
/// unless its owner is one of `ancestors`, the sessions above `by`, and any claim blocks a whole claim or the same box,
/// while two boxes of one issue never refuse each other.
fn blocks(claim: &Claim, by: &str, target: &str, whole: bool, ancestors: &BTreeSet<String>) -> bool {
    claim.owner != by && claim.key() != target && (whole || claim.slug.is_none()) && !(claim.slug.is_none() && !whole && ancestors.contains(&claim.owner))
}

/// Claims the whole issue `item`, or its box `slug`, named as `(item, slug)`, for `by` in one immediate transaction, and returns what it did with the claims key it wrote or refused: a new claim, a renewal of `by`'s own, a
/// replacement of a void claim, a takeover of a stale one, or a refusal by a live one. A box claim refuses beside a live claim on the
/// whole issue, unless the whole issue's owner is above `by`, and a whole claim beside a live claim on any box, while two boxes of one
/// issue never refuse each other; a whole claim by a session that already holds a box of the issue renews that box.
pub fn take(db: &mut Connection, (item, slug): (&str, Option<&str>), Claimant { name: by, ancestors }: Claimant<'_>, eta_min: Option<i64>, issue: Issue, alive: Option<&BTreeSet<String>>, now: i64) -> Result<(Taken, String), String> {
    let at = Timestamp::from_unix_seconds(now).iso();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    let mut statement = tx.prepare(&format!("{CLAIMS} WHERE c.item = ?1 OR substr(c.item, 1, length(?1) + 1) = ?1 || '@'")).map_err(|error| error.to_string())?;
    let on_issue: Vec<Claim> = statement.query_map([item], row).map_err(|error| error.to_string())?.collect::<rusqlite::Result<_>>().map_err(|error| error.to_string())?;
    drop(statement);
    let own_box = on_issue.iter().find(|claim| claim.owner == by && claim.slug.is_some()).filter(|_| slug.is_none());
    let target = own_box.map_or_else(|| key(item, slug), Claim::key);
    let whole = slug.is_none() && own_box.is_none();
    let held = on_issue.iter().find(|claim| claim.key() == target);
    let beside = on_issue.iter().filter(|claim| blocks(claim, by, &target, whole, ancestors)).find(|claim| matches!(claim.standing(issue, alive, now), Standing::Live));
    let taken = decide((held, beside), by, (issue, alive, now));
    let boxed = json!(slug);
    let written = match &taken {
        Taken::Refused(_) => Ok(0),
        Taken::Fresh => tx
            .execute("INSERT INTO claims(item, owner, since, eta_min) VALUES (?1, ?2, ?3, ?4)", params![target, by, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min, "box": boxed}))),
        Taken::Renewed => tx
            .execute("UPDATE claims SET since = ?2, eta_min = COALESCE(?3, eta_min) WHERE item = ?1", params![target, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min, "box": boxed}))),
        Taken::Voided { previous, why } => tx
            .execute("UPDATE claims SET owner = ?2, since = ?3, eta_min = ?4 WHERE item = ?1", params![target, by, at, eta_min])
            .and_then(|_| event(&tx, &at, item, "claim", by, &json!({"eta_min": eta_min, "box": boxed, "void": previous.owner, "why": why}))),
        Taken::Takeover { previous, idle_since } => {
            let idle = Timestamp::from_unix_seconds(*idle_since).iso();
            tx.execute("UPDATE claims SET owner = ?2, since = ?3, eta_min = ?4 WHERE item = ?1", params![target, by, at, eta_min]).and_then(|_| {
                event(&tx, &at, item, "handoff", by, &json!({"from": previous.owner, "to": by, "box": boxed, "takeover": format!("stale: no owner event since {idle}")}))
            })
        }
    };
    written.map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((taken, target))
}

#[cfg(test)]
mod tests {
    use crate::threads::schema;

    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn ledger() -> (tempfile::TempDir, Connection) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("threads.db");
        schema::ensure(&path).unwrap();
        let db = open(&path, true).unwrap();
        (directory, db)
    }

    #[test]
    fn a_child_takes_a_box_of_an_issue_its_lead_holds_whole_and_a_session_outside_the_tree_is_refused() {
        let (_directory, mut db) = ledger();
        let alive = Some(BTreeSet::from(["n-924-child".to_owned(), "n-924-outsider".to_owned(), "qp90-lead".to_owned()]));
        let issue = Issue::Open { opened: NOW - 60 };
        let nought = BTreeSet::new();
        let (lead, _) = take(&mut db, ("north#902", None), Claimant { name: "qp90-lead", ancestors: &nought }, None, issue, alive.as_ref(), NOW).unwrap();
        assert_eq!(lead, Taken::Fresh);

        let beneath = BTreeSet::from(["qp90-lead".to_owned()]);
        let (child, key) = take(&mut db, ("north#902", Some("the-box")), Claimant { name: "n-924-child", ancestors: &beneath }, None, issue, alive.as_ref(), NOW).unwrap();
        assert_eq!((child, key.as_str()), (Taken::Fresh, "north#902@the-box"));
        assert_eq!(
            holders(&db, &["north#902".to_owned()], &[]).unwrap(),
            [("north#902".to_owned(), "qp90-lead".to_owned()), ("north#902@the-box".to_owned(), "n-924-child".to_owned())],
            "the lead keeps the whole issue and the child holds the box it staffs"
        );

        let (outsider, _) = take(&mut db, ("north#902", Some("the-box")), Claimant { name: "n-924-outsider", ancestors: &nought }, None, issue, alive.as_ref(), NOW).unwrap();
        assert!(matches!(outsider, Taken::Refused(claim) if claim.owner == "qp90-lead"));
    }
}
