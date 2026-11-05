//! The runs columns the ingest writes that a ledger created before them lacks, added before the first row.
use rusqlite::Connection;

use crate::threads::verb::Failure;

const ADDED: [(&str, &str); 3] = [
    ("budget_usd", "REAL"),
    ("eta_source", "TEXT CHECK(eta_source IN ('brief', 'tune', 'ledger'))"),
    ("budget_source", "TEXT CHECK(budget_source IN ('brief', 'tune', 'ledger'))"),
];

pub fn ensure(db: &Connection) -> Result<(), Failure> {
    let present: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('runs')")?.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    if present.is_empty() {
        return Ok(());
    }
    for (column, declared) in ADDED.iter().filter(|(column, _)| !present.iter().any(|name| name == column)) {
        db.execute_batch(&format!("ALTER TABLE runs ADD COLUMN {column} {declared}"))?;
    }
    Ok(())
}
