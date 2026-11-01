use rusqlite::Connection;

use super::{PriorityUse, SCHEMA, insert_priority_use, migrate};

#[test]
fn opening_a_current_database_takes_no_write_lock_so_a_held_writer_never_times_it_out() {
    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path().join("capacity.db");
    let first = Connection::open(&path).expect("open");
    first.execute_batch(SCHEMA).expect("schema");
    first.execute_batch("ALTER TABLE capacity_runs ADD COLUMN command TEXT").expect("command column");
    let writer = Connection::open(&path).expect("writer");
    writer.execute_batch("BEGIN IMMEDIATE").expect("hold the write lock");
    let mut reader = Connection::open(&path).expect("reader");
    reader.busy_timeout(std::time::Duration::ZERO).expect("no patience");
    assert!(!migrate(&mut reader).expect("a current schema needs no write lock"));
    writer.execute_batch("ROLLBACK").expect("release");
}

#[test]
fn a_database_missing_the_command_column_is_still_migrated() {
    let scratch = tempfile::tempdir().expect("scratch");
    let mut db = Connection::open(scratch.path().join("capacity.db")).expect("open");
    db.execute_batch("CREATE TABLE capacity_runs (source_id TEXT PRIMARY KEY, at TEXT NOT NULL, family TEXT NOT NULL, repo TEXT, package TEXT, class TEXT NOT NULL, signature_exact INTEGER NOT NULL, cwd TEXT)").expect("old table");
    migrate(&mut db).expect("migration");
    let has_command = db.prepare("PRAGMA table_info(capacity_runs)").expect("pragma").query_map([], |row| row.get::<_, String>(1)).expect("rows").flatten().any(|name| name == "command");
    assert!(has_command);
}

#[test]
fn a_host_database_from_before_priorities_gains_the_priority_table_and_records_each_use_with_its_owner() {
    let scratch = tempfile::tempdir().expect("scratch");
    let mut db = Connection::open(scratch.path().join("capacity.db")).expect("open");
    db.execute_batch(SCHEMA).expect("schema");
    db.execute_batch("ALTER TABLE capacity_runs ADD COLUMN command TEXT; DROP TABLE capacity_priority").expect("a database from before priorities");
    migrate(&mut db).expect("migration");
    let row = PriorityUse { lease: "lease", owner: "wisp-fx-style", class: "heavy", priority: "tom-ask", placed: true, queued_seconds: 4.0, command: "[]" };
    insert_priority_use(&db, &row).expect("recorded");
    let owner: String = db.query_row("SELECT owner FROM capacity_priority WHERE lease = 'lease'", [], |row| row.get(0)).expect("row");
    assert_eq!(owner, "wisp-fx-style");
}
