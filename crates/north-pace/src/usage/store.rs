use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use super::decide::world::ModelFamily;
use super::meter;
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
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn mirrored(&self) -> bool {
        self.snapshot
    }

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

pub(super) fn open_writable(source: &Source, refresh: &dyn Fn()) -> Result<Connection, String> {
    if source.snapshot {
        return open(source, refresh);
    }
    let fail = |error: rusqlite::Error| error.to_string();
    let db = Connection::open(&source.path).map_err(fail)?;
    db.busy_timeout(std::time::Duration::from_secs(10)).map_err(fail)?;
    Ok(db)
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
    let rows: Vec<Row> = rows.collect::<Result<_, _>>().map_err(fail)?;
    Ok(with_fable(rows))
}

/// One Fable weekly meter for every Claude account that has a weekly window but no scoped Fable row.
///
/// The provider reports a model-scoped weekly window only for a family it has opened. Until it does, a
/// Fable session draws on the account's shared weekly window, so that window is the meter: the same row
/// the Opus meter is read from, with its used percent and reset. A reported Fable row always wins.
fn with_fable(mut rows: Vec<Row>) -> Vec<Row> {
    let fable = meter::scoped(ModelFamily::Fable);
    let weekly: std::collections::BTreeMap<String, Value> = rows
        .iter()
        .filter(|row| row.text("provider") == Some("claude") && row.text("kind") == Some("window") && row.text("name") == Some("weekly_all"))
        .filter_map(|row| Some((row.text("account")?.to_owned(), row.value("resets_at")?.clone())))
        .filter(|(_, reset)| !reset_is_empty(reset))
        .collect();
    for row in rows.iter_mut().filter(|row| row.text("kind") == Some("window") && row.text("name") == Some(fable.as_str())) {
        if row.value("resets_at").is_none_or(reset_is_empty)
            && let Some(reset) = row.text("account").and_then(|account| weekly.get(account))
        {
            row.set("resets_at", reset.clone());
        }
    }
    let reported: BTreeSet<String> = rows
        .iter()
        .filter(|row| row.text("kind") == Some("window") && row.text("name") == Some(fable.as_str()))
        .filter_map(|row| row.text("account").map(str::to_owned))
        .collect();
    let fallbacks: Vec<Row> = rows
        .iter()
        .filter(|row| row.text("provider") == Some("claude") && row.text("kind") == Some("window") && row.text("name") == Some("weekly_all"))
        .filter(|row| row.text("account").is_some_and(|account| !reported.contains(account)))
        .filter(|row| row.number("used_pct").is_some())
        .map(fable_fallback)
        .collect();
    rows.extend(fallbacks);
    rows
}

fn reset_is_empty(value: &Value) -> bool {
    matches!(value, Value::Null) || matches!(value, Value::Text(text) if text.is_empty())
}

fn fable_fallback(weekly: &Row) -> Row {
    let source = weekly.text("source").unwrap_or_default();
    let cells = COLUMNS
        .iter()
        .map(|column| match *column {
            "name" => Value::Text(meter::scoped(ModelFamily::Fable)),
            "window_min" => Value::Integer(10080),
            "source" => Value::Text(format!("{source} (Fable weekly meter shares the account's weekly window)")),
            other => weekly.0[COLUMNS.iter().position(|name| *name == other).unwrap_or_default()].clone(),
        })
        .collect();
    Row(cells)
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

    pub(super) fn value(&self, column: &str) -> Option<&Value> {
        self.0.get(COLUMNS.iter().position(|name| *name == column)?)
    }

    pub(super) fn set(&mut self, column: &str, value: Value) {
        if let Some(index) = COLUMNS.iter().position(|name| *name == column) {
            self.0[index] = value;
        }
    }

    pub(super) fn age_minutes(&self, now: Timestamp) -> Result<Option<f64>, String> {
        let Some(ts) = self.text("ts").filter(|ts| !ts.is_empty()) else {
            return Ok(None);
        };
        let read = Timestamp::parse_rfc3339(ts).ok_or_else(|| format!("unreadable reading time {ts:?}"))?;
        Ok(Some((now.unix_nanos() - read.unix_nanos()) as f64 / 1e9 / 60.0))
    }
}
