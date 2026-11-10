use rusqlite::{Connection, OpenFlags};

use super::{SCHEMA, capacity_database_at, migrate, table_columns};

/// The capacity database before the model, priority and admission tables and the run columns the host adds on open.
const OLD_STATE: &str = "CREATE TABLE capacity_runs (
    source_id TEXT PRIMARY KEY, at TEXT NOT NULL, family TEXT NOT NULL, repo TEXT,
    package TEXT, class TEXT NOT NULL, signature_exact INTEGER NOT NULL, cwd TEXT,
    owner TEXT, shape TEXT, reservedCpus REAL, reservedMemoryMiB REAL, chargedCpus REAL,
    chargedMemoryMiB REAL, cpuSeconds REAL, meanCores REAL, p90Cores REAL, peakCores REAL,
    peakMemoryMiB REAL, gpuSeconds REAL, wallSeconds REAL, exitCode INTEGER,
    ioReadBytes REAL, ioWriteBytes REAL, ioPressureSomeUsec REAL, diskBusyShare REAL);
    CREATE INDEX capacity_runs_signature ON capacity_runs(family, repo, package, class, at);";

#[test]
fn an_old_state_capacity_database_opens_writable_and_read_only_and_keeps_its_rows() {
    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path().join("capacity.db");
    let old = Connection::open(&path).expect("old store");
    old.execute_batch(OLD_STATE).expect("the older build's schema");
    old.execute_batch("INSERT INTO capacity_runs (source_id, at, family, class, signature_exact, owner, reservedCpus, meanCores, wallSeconds, exitCode) VALUES ('lease:old', '2026-11-04T10:00:00.000Z', 'cargo', 'heavy', 1, 'north-1022-d0917', 6.0, 2.5, 12.0, 0)")
        .expect("a row the older build wrote");
    drop(old);

    let upgraded = capacity_database_at(&path, false).expect("the writable open");
    assert!(table_columns(&upgraded, "capacity_runs").iter().any(|column| column == "host"), "the writable open adds the run columns the older build lacked");
    let imported: i64 = upgraded.query_row("SELECT count(*) FROM capacity_imports", [], |row| row.get(0)).expect("the import rows");
    assert_eq!(imported, 0, "opening a store of its own reads no usage log");
    let kept: (String, f64) = upgraded.query_row("SELECT owner, meanCores FROM capacity_runs WHERE source_id = 'lease:old'", [], |row| Ok((row.get(0)?, row.get(1)?))).expect("the row the older build wrote");
    assert_eq!(kept, ("north-1022-d0917".to_owned(), 2.5), "the writable open keeps the row the older build wrote");
    drop(upgraded);

    let readonly = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).expect("the read-only open");
    let kept: (String, f64) = readonly.query_row("SELECT owner, wallSeconds FROM capacity_runs WHERE source_id = 'lease:old'", [], |row| Ok((row.get(0)?, row.get(1)?))).expect("the row the older build wrote");
    assert_eq!(kept, ("north-1022-d0917".to_owned(), 12.0), "the read-only open reads the row the older build wrote");
    let created: i64 = readonly.query_row("SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_model'", [], |row| row.get(0)).expect("the model table");
    assert_eq!(created, 1, "the writable open creates the table the older build lacked");
}

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
