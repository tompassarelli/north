use rusqlite::Connection;

const IDENTITY_INDEX: &str = "usage_identity";

pub fn ensure(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT; \
         CREATE INDEX IF NOT EXISTS usage_latest ON usage(provider, account, kind, name, ts)",
    )?;
    let indexed: Option<i64> = db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?", [IDENTITY_INDEX], |row| row.get(0)).ok();
    if indexed.is_some() {
        return Ok(());
    }
    let transaction = db.unchecked_transaction()?;
    let removed = transaction.execute(
        "DELETE FROM usage WHERE rowid NOT IN (SELECT min(rowid) FROM usage GROUP BY ts, provider, ifnull(account, ''), kind, name, ifnull(source, ''))",
        [],
    )?;
    transaction.execute_batch(&format!(
        "CREATE UNIQUE INDEX {IDENTITY_INDEX} ON usage(ts, provider, ifnull(account, ''), kind, name, ifnull(source, ''))"
    ))?;
    transaction.commit()?;
    if removed > 0 {
        db.execute_batch("VACUUM")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "usage_table_tests.rs"]
mod tests;
