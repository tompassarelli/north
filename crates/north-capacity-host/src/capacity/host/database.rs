use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OpenFlags, params_from_iter, types::Value as Sql};
use serde_json::Value;

use super::estimate::{CapacityRun, TaskSignature, task_signature};
use super::machine::{env, read_text};
use super::model::{MODEL_COLUMNS, MODEL_SCHEMA, MODEL_TABLE_COLUMNS};
use super::policy::sizing_runs;

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capacity_runs (
    source_id TEXT PRIMARY KEY, at TEXT NOT NULL, family TEXT NOT NULL, repo TEXT,
    package TEXT, class TEXT NOT NULL, signature_exact INTEGER NOT NULL, cwd TEXT,
    owner TEXT, shape TEXT, reservedCpus REAL, reservedMemoryMiB REAL, chargedCpus REAL,
    chargedMemoryMiB REAL, cpuSeconds REAL, meanCores REAL, p90Cores REAL, peakCores REAL,
    peakMemoryMiB REAL, gpuSeconds REAL, wallSeconds REAL, exitCode INTEGER,
    ioReadBytes REAL, ioWriteBytes REAL, ioPressureSomeUsec REAL, diskBusyShare REAL);
    CREATE INDEX IF NOT EXISTS capacity_runs_signature ON capacity_runs(family, repo, package, class, at);
    CREATE TABLE IF NOT EXISTS capacity_imports (source TEXT PRIMARY KEY, rows INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS capacity_priority (
    lease TEXT PRIMARY KEY, at TEXT NOT NULL, owner TEXT NOT NULL, class TEXT NOT NULL, priority TEXT NOT NULL,
    placed INTEGER NOT NULL, queuedSeconds REAL NOT NULL, command TEXT);
    CREATE TABLE IF NOT EXISTS capacity_admissions (
    id TEXT PRIMARY KEY, at TEXT NOT NULL, owner TEXT NOT NULL, class TEXT NOT NULL, decision TEXT NOT NULL,
    reason TEXT NOT NULL, queuedSeconds REAL NOT NULL, cpus REAL, memoryMiB REAL)";

const MEASUREMENT_COLUMNS: [&str; 22] = [
    "owner", "shape", "reservedCpus", "reservedMemoryMiB", "chargedCpus", "chargedMemoryMiB", "cpuSeconds", "meanCores", "p90Cores", "peakCores",
    "peakMemoryMiB", "gpuSeconds", "wallSeconds", "exitCode", "ioReadBytes", "ioWriteBytes", "ioPressureSomeUsec", "diskBusyShare", "command",
    "host", "shipSeconds", "cacheState",
];

const OFFLOAD_COLUMNS: [(&str, &str); 3] = [("host", "TEXT"), ("shipSeconds", "REAL"), ("cacheState", "TEXT")];

fn state_root() -> PathBuf {
    env("XDG_STATE_HOME").map_or_else(|| Path::new(&env("HOME").unwrap_or_else(|| "/tmp".to_owned())).join(".local/state"), PathBuf::from)
}

pub fn usage_log_path() -> PathBuf {
    env("AGENT_CAPACITY_USAGE_LOG").map_or_else(|| state_root().join("agents/machine-capacity-usage.jsonl"), PathBuf::from)
}

fn private_parent(path: &Path) -> std::io::Result<()> {
    path.parent().map_or(Ok(()), |parent| fs::DirBuilder::new().recursive(true).mode(0o700).create(parent))
}

fn table_columns(db: &Connection, table: &str) -> Vec<String> {
    db.prepare(&format!("PRAGMA table_info({table})")).and_then(|mut statement| statement.query_map([], |row| row.get::<_, String>(1)).map(|names| names.flatten().collect())).unwrap_or_default()
}

fn run_columns() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once(("command", "TEXT")).chain(OFFLOAD_COLUMNS).chain(MODEL_COLUMNS)
}

fn add_columns(db: &Connection, table: &str, columns: impl Iterator<Item = (&'static str, &'static str)>) -> rusqlite::Result<()> {
    let present = table_columns(db, table);
    for (name, kind) in columns {
        if !present.iter().any(|column| column == name) {
            db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {name} {kind}"))?;
        }
    }
    Ok(())
}

pub(super) fn add_run_columns(db: &Connection) -> rusqlite::Result<()> {
    add_columns(db, "capacity_runs", run_columns())
}

fn has_columns(db: &Connection) -> bool {
    let holds = |table: &str, wanted: Vec<(&str, &str)>| {
        let present = table_columns(db, table);
        wanted.iter().all(|(name, _)| present.iter().any(|column| column == name))
    };
    holds("capacity_runs", run_columns().collect()) && holds("capacity_model", MODEL_TABLE_COLUMNS.to_vec())
}

/// Whether the schema is already in place, read without a write lock: every capacity command opens the database, and a write transaction per open queued forty queued wrappers behind each other's history reads past the busy timeout when a runtime update re-ran them at once.
fn current(db: &Connection) -> bool {
    let named = |kind: &str, name: &str| db.query_row("SELECT 1 FROM sqlite_master WHERE type = ? AND name = ?", [kind, name], |_| Ok(())).is_ok();
    has_columns(db) && named("table", "capacity_imports") && named("table", "capacity_priority") && named("table", "capacity_admissions") && named("table", "capacity_model") && named("index", "capacity_runs_signature")
}

pub(super) fn migrate(db: &mut Connection) -> rusqlite::Result<bool> {
    if current(db) {
        return Ok(false);
    }
    let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if current(&transaction) {
        return Ok(false);
    }
    let first_open = transaction.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'capacity_runs'", [], |_| Ok(())).is_err();
    let legacy = state_root().join("threads/threads.db");
    let attached = first_open && legacy.exists();
    if attached {
        let uri = format!("file:{}?mode=ro", legacy.display());
        transaction.execute("ATTACH DATABASE ? AS legacy_capacity", [uri])?;
    }
    transaction.execute_batch(SCHEMA)?;
    if attached {
        for table in ["capacity_runs", "capacity_imports"] {
            let present = transaction.query_row("SELECT 1 FROM legacy_capacity.sqlite_master WHERE type = 'table' AND name = ?", [table], |_| Ok(())).is_ok();
            if present {
                transaction.execute_batch(&format!("INSERT OR IGNORE INTO {table} SELECT * FROM legacy_capacity.{table}"))?;
            }
        }
    }
    add_run_columns(&transaction)?;
    transaction.execute_batch(MODEL_SCHEMA)?;
    add_columns(&transaction, "capacity_model", MODEL_TABLE_COLUMNS.into_iter())?;
    transaction.commit()?;
    Ok(attached)
}

fn import_usage_log(db: &mut Connection) -> rusqlite::Result<()> {
    let source = usage_log_path().to_string_lossy().into_owned();
    if db.query_row("SELECT rows FROM capacity_imports WHERE source = ?", [&source], |_| Ok(())).is_ok() {
        return Ok(());
    }
    let text = read_text(&source).unwrap_or_default();
    let transaction = db.transaction()?;
    let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
    for (index, line) in lines.iter().enumerate() {
        let entry: Value = serde_json::from_str(line).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let shape: Vec<String> = entry["shape"].as_str().unwrap_or_default().split_whitespace().map(str::to_owned).collect();
        let signature = task_signature(&shape, None, entry["class"].as_str().unwrap_or_default(), false);
        insert_capacity_run(&transaction, &format!("jsonl:{source}:{index}"), &entry, &signature, None)?;
    }
    transaction.execute("INSERT INTO capacity_imports VALUES (?, ?)", rusqlite::params![source, lines.len() as i64])?;
    transaction.commit()
}

pub const TEMPORARY_FAILURE: i32 = north_base::foundation::exit::NO_VERDICT_STATUS;

const LOCK_WAIT_SECONDS: f64 = 60.0;
const RETRY_PAUSE: Duration = Duration::from_millis(50);

fn lock_wait() -> Duration {
    let seconds = env("AGENT_CAPACITY_DB_LOCK_WAIT_SECONDS").and_then(|text| text.parse::<f64>().ok()).filter(|seconds| seconds.is_finite() && *seconds >= 0.0).unwrap_or(LOCK_WAIT_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn is_contended(error: &rusqlite::Error) -> bool {
    matches!(error.sqlite_error_code(), Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
}

pub fn failure_status(error: &rusqlite::Error) -> i32 {
    if is_contended(error) { TEMPORARY_FAILURE } else { 1 }
}

fn open_once(path: &Path, wait: Duration, seed: bool) -> rusqlite::Result<Connection> {
    let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI)?;
    db.busy_timeout(wait)?;
    if migrate(&mut db)? {
        db.execute_batch("DETACH DATABASE legacy_capacity")?;
    }
    if seed {
        import_usage_log(&mut db)?;
    }
    Ok(db)
}

/// The capacity database at `path`, created and migrated, then seeded from this host's usage log on first open when
/// `seed` is set; SQLite skips its busy handler when a read transaction upgrades to a write, so the whole open is
/// retried until the lock wait ends. A caller that opens a store of its own, such as an old-state fixture, passes
/// `seed: false` so the open reads no host state.
fn opening(path: &Path, seed: bool) -> rusqlite::Result<Connection> {
    let deadline = Instant::now() + lock_wait();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match open_once(path, remaining, seed) {
            Err(error) if is_contended(&error) && Instant::now() < deadline => std::thread::sleep(RETRY_PAUSE),
            other => return other,
        }
    }
}

/// The capacity database at `path`, created and migrated; `seed` reads this host's usage log into it on first open.
pub fn capacity_database_at(path: &Path, seed: bool) -> rusqlite::Result<Connection> {
    private_parent(path).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    opening(path, seed)
}

/// The capacity database, created and seeded from the usage log on first open.
pub fn capacity_database() -> rusqlite::Result<Connection> {
    let path = env("AGENT_CAPACITY_DB").map_or_else(|| state_root().join("agents/machine-capacity/capacity.db"), PathBuf::from);
    capacity_database_at(&path, true)
}

pub(super) fn sql(value: Option<&Value>) -> Sql {
    match value {
        None | Some(Value::Null) => Sql::Null,
        Some(Value::Bool(flag)) => Sql::Integer(i64::from(*flag)),
        Some(Value::Number(number)) => number.as_i64().map_or_else(|| Sql::Real(number.as_f64().unwrap_or(f64::NAN)), Sql::Integer),
        Some(Value::String(text)) => Sql::Text(text.clone()),
        Some(other) => Sql::Text(other.to_string()),
    }
}

pub fn insert_capacity_run(db: &Connection, source: &str, entry: &Value, signature: &TaskSignature, cwd: Option<&str>) -> rusqlite::Result<()> {
    let columns: Vec<&str> = ["source_id", "at", "family", "repo", "package", "class", "signature_exact", "cwd"].into_iter().chain(MEASUREMENT_COLUMNS).chain(MODEL_COLUMNS.map(|(name, _)| name)).collect();
    let placeholders = vec!["?"; columns.len()].join(",");
    let text = |value: &Option<String>| value.clone().map_or(Sql::Null, Sql::Text);
    let mut values = vec![
        Sql::Text(source.to_owned()),
        sql(entry.get("at")),
        Sql::Text(signature.family.clone()),
        text(&signature.repo),
        text(&signature.package),
        Sql::Text(signature.class.clone()),
        Sql::Integer(i64::from(signature.exact)),
        cwd.map_or(Sql::Null, |cwd| Sql::Text(cwd.to_owned())),
    ];
    values.extend(MEASUREMENT_COLUMNS.iter().copied().chain(MODEL_COLUMNS.map(|(name, _)| name)).map(|key| sql(entry.get(key))));
    db.execute(&format!("INSERT OR IGNORE INTO capacity_runs ({}) VALUES ({placeholders})", columns.join(",")), params_from_iter(values))?;
    Ok(())
}

/// One admitted run that asked for a priority, and whether it was placed first or, past the cap, kept its arrival place.
pub struct PriorityUse<'a> {
    pub lease: &'a str,
    pub owner: &'a str,
    pub class: &'a str,
    pub priority: &'a str,
    pub placed: bool,
    pub queued_seconds: f64,
    pub command: &'a str,
}

pub fn insert_priority_use(db: &Connection, row: &PriorityUse<'_>) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO capacity_priority (lease, at, owner, class, priority, placed, queuedSeconds, command) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![row.lease, crate::foundation::clock::Timestamp::now().iso_millis(), row.owner, row.class, row.priority, row.placed, row.queued_seconds, row.command],
    )?;
    Ok(())
}

/// How long a host keeps an admission it has not yet shipped to the ledger.
const ADMISSIONS_KEPT_SECONDS: i64 = 35 * 86_400;

/// One admission's outcome: the lease it reserved, or the deferral or refusal that ended its wait.
pub struct Admission<'a> {
    pub id: &'a str,
    pub owner: &'a str,
    pub class: &'a str,
    pub decision: &'a str,
    pub reason: &'a str,
    pub queued_seconds: f64,
    pub cpus: Option<f64>,
    pub memory_mib: Option<f64>,
}

pub fn insert_admission(db: &Connection, row: &Admission<'_>) -> rusqlite::Result<()> {
    let now = crate::foundation::clock::Timestamp::now();
    db.execute("DELETE FROM capacity_admissions WHERE at < ?", [crate::foundation::clock::Timestamp::from_unix_seconds(now.unix_seconds() - ADMISSIONS_KEPT_SECONDS).iso_millis()])?;
    db.execute(
        "INSERT OR IGNORE INTO capacity_admissions (id, at, owner, class, decision, reason, queuedSeconds, cpus, memoryMiB) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![row.id, now.iso_millis(), row.owner, row.class, row.decision, row.reason, row.queued_seconds, row.cpus, row.memory_mib],
    )?;
    Ok(())
}

/// The ledger rows of the admissions recorded at or after `since` (all when none), oldest first.
pub fn admission_rows(db: &Connection, host: &str, since: Option<&str>) -> rusqlite::Result<Vec<Value>> {
    let mut statement = db.prepare("SELECT id, at, owner, class, decision, reason, queuedSeconds, cpus, memoryMiB FROM capacity_admissions WHERE at >= ? ORDER BY at, id")?;
    let rows = statement.query_map([since.unwrap_or("")], |row| {
        Ok(serde_json::json!({
            "type": "capacity", "host": host, "id": row.get::<_, String>(0)?, "at": row.get::<_, String>(1)?, "owner": row.get::<_, String>(2)?,
            "class": row.get::<_, String>(3)?, "decision": row.get::<_, String>(4)?, "reason": row.get::<_, String>(5)?,
            "queued_seconds": row.get::<_, f64>(6)?, "cpus": row.get::<_, Option<f64>>(7)?, "memory_mib": row.get::<_, Option<f64>>(8)?,
        }))
    })?;
    rows.collect()
}

/// The longest a session's gated run may queue for admission before the wait counts as the harness holding it, not the worker.
pub const QUEUED_BOUND_SECONDS: f64 = 300.0;

/// A gated run admitted after queueing for capacity: when it was admitted (unix seconds) and how long it queued.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub at: i64,
    pub seconds: f64,
}

/// The gated runs of fleet session `session` admitted between `from` and `to` (unix seconds) that queued at least
/// `QUEUED_BOUND_SECONDS`, longest first, read from the host's `capacity_priority` rows (owner `gate:SESSION`). A database
/// without the table, or a host that never admitted the session, answers none.
pub fn queued_behind_capacity(db: &Connection, session: &str, from: i64, to: i64) -> Vec<Queued> {
    let Ok(mut statement) = db.prepare("SELECT at, queuedSeconds FROM capacity_priority WHERE owner = ?1 AND queuedSeconds >= ?2") else { return Vec::new() };
    let rows = statement.query_map(rusqlite::params![format!("gate:{session}"), QUEUED_BOUND_SECONDS], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)));
    let mut queued: Vec<Queued> = rows
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|(at, seconds)| Some(Queued { at: crate::foundation::clock::Timestamp::parse(&at).ok()?.unix_seconds(), seconds }))
        .filter(|queued| queued.at >= from && queued.at <= to)
        .collect();
    queued.sort_by(|left, right| right.seconds.total_cmp(&left.seconds));
    queued
}

/// The gated runs admitted since `since` (unix seconds) that queued at least `QUEUED_BOUND_SECONDS`, by the fleet session
/// that owns each (owner `gate:SESSION`), for the resume pass that wakes a session its queue admitted.
pub fn queued_admissions(db: &Connection, since: i64) -> std::collections::BTreeMap<String, Vec<Queued>> {
    let mut admitted: std::collections::BTreeMap<String, Vec<Queued>> = std::collections::BTreeMap::new();
    let Ok(mut statement) = db.prepare("SELECT owner, at, queuedSeconds FROM capacity_priority WHERE owner LIKE 'gate:%' AND queuedSeconds >= ?1") else { return admitted };
    let rows = statement.query_map(rusqlite::params![QUEUED_BOUND_SECONDS], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?)));
    for (owner, at, seconds) in rows.into_iter().flatten().filter_map(Result::ok) {
        let Some(session) = owner.strip_prefix("gate:") else { continue };
        let Ok(at) = crate::foundation::clock::Timestamp::parse(&at).map(|at| at.unix_seconds()) else { continue };
        if at >= since {
            admitted.entry(session.to_owned()).or_default().push(Queued { at, seconds });
        }
    }
    admitted
}

/// The capacity database opened read-only where it exists, for a reader that must not create it.
pub fn capacity_database_readonly() -> Option<Connection> {
    let path = env("AGENT_CAPACITY_DB").map_or_else(|| state_root().join("agents/machine-capacity/capacity.db"), PathBuf::from);
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()
}

fn real(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<Option<f64>> {
    row.get::<_, Option<f64>>(column)
}

fn capacity_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<CapacityRun> {
    Ok(CapacityRun {
        at: row.get("at")?,
        signature: TaskSignature {
            family: row.get("family")?,
            repo: row.get("repo")?,
            package: row.get("package")?,
            class: row.get("class")?,
            exact: row.get::<_, i64>("signature_exact")? != 0,
        },
        owner: row.get("owner")?,
        shape: row.get("shape")?,
        mean_cores: real(row, "meanCores")?,
        p90_cores: real(row, "p90Cores")?,
        peak_memory_mib: real(row, "peakMemoryMiB")?,
        io_read_bytes: real(row, "ioReadBytes")?,
        io_write_bytes: real(row, "ioWriteBytes")?,
        disk_busy_share: real(row, "diskBusyShare")?,
        wall_seconds: real(row, "wallSeconds")?,
        exit_code: real(row, "exitCode")?,
        host: row.get("host")?,
    })
}

pub fn capacity_history(db: &Connection) -> rusqlite::Result<Vec<CapacityRun>> {
    capacity_history_of(db, &crate::platform::host::name())
}

pub(super) fn capacity_history_of(db: &Connection, local: &str) -> rusqlite::Result<Vec<CapacityRun>> {
    let has_host = super::fleet_pull::run_columns(db).iter().any(|name| name == "host");
    let filter = if has_host { "WHERE host IS NULL OR host = ?" } else { "WHERE ?1 IS NOT NULL" };
    let mut statement = db.prepare(&format!("SELECT * FROM capacity_runs {filter} ORDER BY at, source_id"))?;
    statement.query_map([local], capacity_run)?.collect()
}

pub fn record_usage(entry: &str) -> std::io::Result<()> {
    let path = usage_log_path();
    private_parent(&path)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&path)?;
    file.write_all(format!("{entry}\n").as_bytes())
}

/// Where the runs the lander's own leases made are kept, one file a day: under the state home's `north`, which no worker command writes, unlike the usage log, so only those leases set a landing command's charge.
fn lander_runs_directory() -> PathBuf {
    state_root().join("north/capacity/lander-runs")
}

fn day_file(at: &str) -> String {
    format!("{}.jsonl", at.get(..10).unwrap_or(at))
}

/// Appends `entry`, a run a lander's lease made at the ISO time `at`, to its day's file, and removes the files of the days before the ISO time `keep`.
pub fn record_lander_run(entry: &str, at: &str, keep: &str) -> std::io::Result<()> {
    let directory = lander_runs_directory();
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory)?;
    let stale = day_file(keep);
    for old in fs::read_dir(&directory)?.flatten().filter(|file| *file.file_name().to_string_lossy() < *stale) {
        let _ = fs::remove_file(old.path());
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).mode(0o600).open(directory.join(day_file(at)))?;
    file.write_all(format!("{entry}\n").as_bytes())
}

/// The lines the lander's leases recorded on the day of the ISO time `since` and after it.
pub fn lander_runs(since: &str) -> String {
    let first = day_file(since);
    let mut days: Vec<PathBuf> = fs::read_dir(lander_runs_directory()).into_iter().flatten().flatten().filter(|file| *file.file_name().to_string_lossy() >= *first).map(|file| file.path()).collect();
    days.sort();
    days.iter().filter_map(read_text).collect()
}

/// The latest sizing window of the usage log's last lines that match.
pub fn recent_usage(lines: usize, matches: impl Fn(&Value) -> bool) -> Vec<Value> {
    let text = read_text(usage_log_path()).unwrap_or_default();
    if env("AGENT_CAPACITY_USAGE_LOG").is_none() {
        crate::platform::reader_stamp::touch(&state_root(), "agents/machine-capacity-usage.jsonl");
    }
    let host = crate::capacity::local_machine::host_name();
    let all: Vec<&str> = text.split('\n').collect();
    let matching: Vec<Value> = all[all.len().saturating_sub(lines)..]
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry.get("host").and_then(Value::as_str).is_none_or(|recorded| recorded == host) && matches(entry))
        .collect();
    let keep = matching.len().saturating_sub(sizing_runs());
    matching.into_iter().skip(keep).collect()
}

#[cfg(test)]
#[path = "database_tests.rs"]
mod tests;

/// One run that was offloaded to this host, as its record stores it.
pub struct OffloadRow {
    pub at: String,
    pub host: Option<String>,
    pub owner: Option<String>,
    pub ship_seconds: f64,
    pub cache_state: Option<String>,
    pub wall_seconds: Option<f64>,
}

/// The newest `last` runs that carry a ship time, newest first.
pub fn offload_rows(db: &Connection, last: usize) -> rusqlite::Result<Vec<OffloadRow>> {
    let mut statement = db.prepare("SELECT at, host, owner, shipSeconds, cacheState, wallSeconds FROM capacity_runs WHERE shipSeconds IS NOT NULL ORDER BY at DESC, source_id DESC LIMIT ?")?;
    statement
        .query_map([i64::try_from(last).unwrap_or(i64::MAX)], |row| {
            Ok(OffloadRow { at: row.get(0)?, host: row.get(1)?, owner: row.get(2)?, ship_seconds: row.get(3)?, cache_state: row.get(4)?, wall_seconds: row.get(5)? })
        })?
        .collect()
}
