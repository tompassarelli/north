//! `north threads control-plane read|cas`: the lease row that names the host running the control plane, in the lease
//! database beside the ledger rather than in it. The row is created on first write; on the ledger host, a read of a
//! database that has none sees the seed, and a replica refuses to read. Only a compare-and-swap on the epoch changes
//! it, so two holders never both hold the lease.
use std::fs::OpenOptions;
use std::io::{Read, Write};

use rusqlite::{Connection, OptionalExtension, params};

use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};
use north_control_plane::{Lease, SEED_EPOCH, SEED_HOLDER, database, well_formed};

pub const VERBS: [&str; 1] = ["control-plane"];

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS control_lease(
  id INTEGER PRIMARY KEY CHECK(id = 1),
  holder TEXT NOT NULL,
  epoch INTEGER NOT NULL CHECK(epoch >= 0),
  renewed_at TEXT NOT NULL
) STRICT;";

const USAGE: &str = "usage: threads control-plane read | cas --epoch N --holder HOST";

pub fn run(argv: &[String], context: &Context, _: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv.get(1).map(String::as_str) {
        Some("read") => read(context),
        Some("cas") => cas(&argv[2..], context),
        _ => Err(Failure::Usage(USAGE.into())),
    };
    report(&argv[0], result, out, err)
}

fn read(context: &Context) -> Outcome {
    if context.replica {
        return Err(Failure::Refused("the control-plane lease is read from the ledger host: this replica holds no copy of it, so it refuses rather than read the seed".into()));
    }
    let path = database();
    let db = if path.exists() { open(&path, false)? } else { Connection::open_in_memory()? };
    Ok(line(&read_on(&db)?))
}

fn cas(argv: &[String], context: &Context) -> Outcome {
    if context.replica {
        return Err(Failure::Refused("the control-plane lease is written only on the ledger host, not on a replica".into()));
    }
    let args = Args::parse(argv, &["--epoch", "--holder"], &[])?;
    let epoch = args.get("--epoch").and_then(|text| text.parse::<i64>().ok()).ok_or_else(|| Failure::Usage("--epoch N is required".into()))?;
    let holder = args.get("--holder").filter(|holder| !holder.is_empty()).ok_or_else(|| Failure::Usage("--holder HOST is required".into()))?;
    if !well_formed(holder) {
        return Err(Failure::Usage(format!("--holder {holder} is no host name, nor a move between two: the lease takes letters, digits and dashes only")));
    }
    let path = database();
    let lease_directory = path.parent().ok_or_else(|| Failure::Refused(format!("the lease database {} has no directory", path.display())))?;
    std::fs::create_dir_all(lease_directory).map_err(|error| Failure::Refused(format!("lease directory {}: {error}", lease_directory.display())))?;
    OpenOptions::new().create(true).append(true).open(&path).map_err(|error| Failure::Refused(format!("lease database {}: {error}", path.display())))?;
    let mut db = open(&path, true)?;
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    let row = compare_and_swap(&tx, epoch, holder, &context.now.iso())?;
    tx.commit()?;
    Ok(line(&row))
}

/// Writes `holder` at epoch + 1 when the stored epoch is `epoch`. A stale epoch changes no row and is refused with the epoch found.
pub fn compare_and_swap(db: &Connection, epoch: i64, holder: &str, now: &str) -> Result<Row, Failure> {
    db.execute("INSERT OR IGNORE INTO control_lease(id, holder, epoch, renewed_at) VALUES (1, ?1, ?2, ?3)", params![SEED_HOLDER, SEED_EPOCH, now])?;
    let changed = db.execute("UPDATE control_lease SET holder = ?1, epoch = epoch + 1, renewed_at = ?2 WHERE id = 1 AND epoch = ?3", params![holder, now, epoch])?;
    let row = read_on(db)?;
    if changed == 0 {
        return Err(Failure::Refused(format!("the lease is at epoch {} held by {}, not epoch {epoch}: read it again", row.lease.epoch, row.lease.holder)));
    }
    Ok(row)
}

/// The lease row with its renewal stamp, or the seed when the ledger has no row yet.
pub fn read_on(db: &Connection) -> Result<Row, Failure> {
    let table: bool = db.query_row("SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'control_lease'", [], |row| row.get(0))?;
    let stored = if table {
        db.query_row("SELECT holder, epoch, renewed_at FROM control_lease WHERE id = 1", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).optional()?
    } else {
        None
    };
    Ok(match stored {
        Some((holder, epoch, renewed_at)) => Row { lease: Lease { holder, epoch }, renewed_at },
        None => Row { lease: Lease { holder: SEED_HOLDER.to_owned(), epoch: SEED_EPOCH }, renewed_at: String::new() },
    })
}

pub struct Row {
    pub lease: Lease,
    pub renewed_at: String,
}

fn line(row: &Row) -> String {
    format!("holder={} epoch={} renewed_at={}\n", row.lease.holder, row.lease.epoch, row.renewed_at)
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::foundation::clock::Timestamp;
    use crate::threads::fake::Recorded;
    use crate::threads::verb::Context;

    #[test]
    fn a_replica_without_the_lease_refuses_to_read_and_never_prints_the_seed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.db");
        let outside = Recorded::default();
        let context = Context { database: &path, actor: "tester".into(), no_native: None, outside: &outside, now: Timestamp::parse("2026-11-15T09:00:00Z").unwrap(), replica: true };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&["control-plane".to_owned(), "read".to_owned()], &context, &mut std::io::empty(), &mut out, &mut err);
        assert_ne!(code, 0, "a replica read a lease");
        assert!(out.is_empty(), "a replica printed the seed: {}", String::from_utf8_lossy(&out));
        assert!(String::from_utf8_lossy(&err).contains("ledger host"), "the refusal does not name the ledger host: {}", String::from_utf8_lossy(&err));
    }
}
