//! The runs columns the ingest writes that a ledger created before them lacks, and the index its landing join probes, added before the first row.
use rusqlite::Connection;

use crate::threads::verb::Failure;

const ADDED: [(&str, &str); 7] = [
    ("box", "TEXT"),
    ("idle_compaction", "INTEGER CHECK(idle_compaction IN (0, 1))"),
    ("fleet_name", "TEXT"),
    ("stop", "TEXT"),
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
    db.execute_batch("CREATE INDEX IF NOT EXISTS runs_fleet_name ON runs(fleet_name, started)")?;
    Ok(())
}
