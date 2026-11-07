//! The usage line of `north ledger flow`: usage per first try landed and usage stranded over the last 24 h, beside the harness losses.
use rusqlite::{Connection, OpenFlags};

use super::host::Places;
use crate::threads::priority_log;
use crate::threads::usage_yield::{Facts, Sources};

const DAY_S: i64 = 86_400;

pub fn line(places: &Places, now: i64) -> String {
    let db = match Connection::open_with_flags(&places.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) {
        Ok(db) => db,
        Err(why) => return format!("usage per landing 24 h: unread: {}: {why}", places.ledger.display()),
    };
    let policy = crate::platform::orchestration::path();
    let facts = Facts::load(&Sources { db: &db, policy: &policy, state: &places.state }, now);
    let waited = priority_log::waited(&priority_log::read(&places.state), now - DAY_S, now + 1).unwrap_or(false);
    let reading = facts.reading(now - DAY_S, now + 1, waited);
    let mut line = reading.line(24);
    if !facts.unread.is_empty() {
        line += &format!("; unread: {}", facts.unread.join("; "));
    }
    line
}
