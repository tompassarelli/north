use rusqlite::Connection;

use super::{SCHEMA, migrate};

// KEEP(bug): opening a current capacity database takes no write lock; regression of 3586f7b0a (every capacity command waited behind history reads)
#[test]
fn opening_a_current_database_takes_no_write_lock_so_a_held_writer_never_times_it_out() {
    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path().join("capacity.db");
    let mut first = Connection::open(&path).expect("open");
    migrate(&mut first).expect("current schema");
    let writer = Connection::open(&path).expect("writer");
    writer.execute_batch("BEGIN IMMEDIATE").expect("hold the write lock");
    let mut reader = Connection::open(&path).expect("reader");
    reader.busy_timeout(std::time::Duration::ZERO).expect("no patience");
    assert!(!migrate(&mut reader).expect("a current schema needs no write lock"));
    writer.execute_batch("ROLLBACK").expect("release");
}

// KEEP(contract): persisted capacity.db migrates from the fleet-pull shape and fresh to the model schema without losing rows
#[test]
fn a_database_written_by_the_fleet_pull_and_a_fresh_one_both_migrate_to_the_model_schema() {
    use super::table_columns;
    let scratch = tempfile::tempdir().expect("scratch");
    let mut shaped = Connection::open(scratch.path().join("shaped.db")).expect("open");
    shaped.execute_batch(SCHEMA).expect("schema");
    shaped
        .execute_batch("ALTER TABLE capacity_runs ADD COLUMN command TEXT; ALTER TABLE capacity_runs ADD COLUMN host TEXT; ALTER TABLE capacity_runs ADD COLUMN shipSeconds REAL; ALTER TABLE capacity_runs ADD COLUMN cacheState TEXT; ALTER TABLE capacity_runs ADD COLUMN queuedSeconds REAL")
        .expect("a database as the fleet pull left it");
    shaped
        .execute_batch("INSERT INTO capacity_runs (source_id, at, family, class, signature_exact, host, shipSeconds, cacheState, queuedSeconds) VALUES ('lease:old', '2026-11-04T10:00:00.000Z', 'cargo', 'heavy', 0, 'gex45', 3.0, 'warm', 4.5)")
        .expect("a row written before the model");
    shaped
        .execute_batch("CREATE TABLE capacity_model (key TEXT NOT NULL, host TEXT NOT NULL, cache TEXT NOT NULL, runs INTEGER NOT NULL, wall TEXT NOT NULL, cpu TEXT NOT NULL, memory TEXT NOT NULL, floorMiB REAL, PRIMARY KEY (key, host, cache))")
        .expect("a model written before censored durations");
    let mut fresh = Connection::open(scratch.path().join("fresh.db")).expect("open");
    assert!(migrate(&mut shaped).is_ok() && migrate(&mut fresh).is_ok());
    assert!(!migrate(&mut shaped).expect("a second migration"));
    for db in [&shaped, &fresh] {
        let columns = table_columns(db, "capacity_runs");
        for name in ["command", "host", "shipSeconds", "cacheState", "key", "cache", "queuedSeconds", "anonPeakMiB", "memoryStallUsec", "censored", "predictedWallSeconds", "predictedWallP90Seconds", "predictedMemoryMiB", "predictionBasis"] {
            assert_eq!(columns.iter().filter(|column| column.as_str() == name).count(), 1, "{name}");
        }
        assert!(db.prepare("SELECT key, host, cache, runs, wallCensored FROM capacity_model").is_ok());
    }
    let kept: String = shaped.query_row("SELECT cacheState FROM capacity_runs WHERE source_id = 'lease:old'", [], |row| row.get(0)).expect("the old row");
    assert_eq!(kept, "warm");
    let queued: f64 = shaped.query_row("SELECT queuedSeconds FROM capacity_runs WHERE source_id = 'lease:old'", [], |row| row.get(0)).expect("the old row");
    assert_eq!(queued, 4.5);
}
