use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use crate::foundation::clock::Timestamp;

pub(super) const COLUMNS: [&str; 10] =
    ["ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source"];

const LATEST: &str = "SELECT ts, provider, account, kind, name, used_pct, window_min, resets_at, amount, source FROM usage \
    WHERE rowid IN (SELECT (SELECT rowid FROM usage WHERE provider = k.provider AND account = k.account \
    AND kind = k.kind AND name = k.name ORDER BY ts DESC, rowid DESC LIMIT 1) \
    FROM (SELECT DISTINCT provider, account, kind, name FROM usage) k) \
    ORDER BY provider, account, kind DESC, window_min, name";

pub(super) struct Source {
    path: PathBuf,
    snapshot: bool,
}

pub(super) struct Row(Vec<Value>);

impl Source {
    pub(super) fn locate(home: &Path, explicit: Option<OsString>) -> Self {
        let snapshot = explicit.is_none() && home.join(".config/threads/remote").exists();
        let path = explicit.map_or_else(
            || home.join(".local/state/threads").join(if snapshot { "snapshot.db" } else { "threads.db" }),
            PathBuf::from,
        );
        Self { path, snapshot }
    }
}

pub(super) fn open(source: &Source, refresh: &dyn Fn()) -> Result<Connection, String> {
    let fail = |error: rusqlite::Error| error.to_string();
    if !source.path.exists() {
        return Connection::open_in_memory().map_err(fail);
    }
    if !source.snapshot {
        return Connection::open_with_flags(&source.path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail);
    }
    match snapshot_copy(&source.path) {
        Ok(copy) => Ok(copy),
        Err(_) => {
            refresh();
            snapshot_copy(&source.path).map_err(|error| format!("NorthThreads: snapshot still unreadable after one refresh: {error}"))
        }
    }
}

fn snapshot_copy(path: &Path) -> Result<Connection, String> {
    let fail = |error: rusqlite::Error| error.to_string();
    let immutable = path.file_name().is_some_and(|name| name == "snapshot.db");
    let uri = format!("file:{}?mode=ro{}", path.display(), if immutable { "&immutable=1" } else { "" });
    let ledger = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(fail)?;
    let verdict: String = ledger.query_row("PRAGMA quick_check", [], |row| row.get(0)).map_err(fail)?;
    if verdict != "ok" {
        return Err("snapshot integrity check failed".into());
    }
    let mut memory = Connection::open_in_memory().map_err(fail)?;
    rusqlite::backup::Backup::new(&ledger, &mut memory).and_then(|backup| backup.run_to_completion(64, std::time::Duration::ZERO, None)).map_err(fail)?;
    Ok(memory)
}

pub(super) fn latest(db: &Connection) -> Result<Vec<Row>, String> {
    let fail = |error: rusqlite::Error| error.to_string();
    let table: Option<i64> = db.query_row("SELECT 1 FROM sqlite_master WHERE name = 'usage'", [], |row| row.get(0)).ok();
    if table.is_none() {
        return Ok(Vec::new());
    }
    let mut query = db.prepare(LATEST).map_err(fail)?;
    let rows = query
        .query_map([], |row| (0..COLUMNS.len()).map(|index| row.get::<_, Value>(index)).collect::<Result<Vec<_>, _>>().map(Row))
        .map_err(fail)?;
    rows.collect::<Result<_, _>>().map_err(fail)
}

impl Row {
    pub(super) fn text(&self, column: &str) -> Option<&str> {
        match &self.0[COLUMNS.iter().position(|name| *name == column)?] {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(super) fn number(&self, column: &str) -> Option<f64> {
        match &self.0[COLUMNS.iter().position(|name| *name == column)?] {
            Value::Real(number) => Some(*number),
            Value::Integer(number) => Some(*number as f64),
            _ => None,
        }
    }

    pub(super) fn values(&self) -> impl Iterator<Item = (&'static str, &Value)> {
        COLUMNS.iter().copied().zip(&self.0)
    }

    pub(super) fn age_minutes(&self, now: Timestamp) -> Result<Option<f64>, String> {
        let Some(ts) = self.text("ts").filter(|ts| !ts.is_empty()) else {
            return Ok(None);
        };
        let read = Timestamp::parse_rfc3339(ts).ok_or_else(|| format!("unreadable reading time {ts:?}"))?;
        Ok(Some((now.unix_nanos() - read.unix_nanos()) as f64 / 1e9 / 60.0))
    }
}
