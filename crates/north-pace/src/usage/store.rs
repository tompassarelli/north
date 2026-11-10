use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use super::decide::world::{Label, LimitEvent, ModelFamily, Percent, Reading, RowId, Source as Via, Unix};
use super::meter;
use crate::foundation::clock::Timestamp;

pub(super) const COLUMNS: [&str; 10] =
    ["ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source"];

const LATEST: &str = "SELECT rowid, ts, provider, account, kind, name, used_pct, window_min, resets_at, amount, source FROM usage \
    WHERE rowid IN (SELECT (SELECT rowid FROM usage WHERE provider = k.provider AND account = k.account \
    AND kind = k.kind AND name = k.name ORDER BY ts DESC, rowid DESC LIMIT 1) \
    FROM (SELECT DISTINCT provider, account, kind, name FROM usage) k) \
    ORDER BY provider, account, kind DESC, window_min, name";

/// A window at or above this percent reads full.
pub const FULL_PCT: f64 = 99.5;

pub(super) struct Source {
    path: PathBuf,
    snapshot: bool,
}

pub(super) struct Row {
    cells: Vec<Value>,
    id: i64,
}

/// A window whose latest reading reads full is also a limit event, which stays true however old the reading grows.
#[derive(Default)]
pub struct Windows {
    pub readings: Vec<Reading>,
    pub limits: Vec<LimitEvent>,
}

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
        .query_map([], |row| Ok(Row { id: row.get(0)?, cells: (0..COLUMNS.len()).map(|index| row.get::<_, Value>(index + 1)).collect::<Result<Vec<_>, _>>()? }))
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
    rows.extend(fallbacks.into_iter().enumerate().map(|(index, row)| Row { id: -(index as i64) - 1, ..row }));
    rows
}

/// The latest reading of every Claude window, read from the one latest-reading query, with each window that reads
/// full until a reset still ahead of `now_ms` also as a limit event.
pub(super) fn latest_windows(db: &Connection, now_ms: u64) -> Windows {
    let mut windows = Windows::default();
    let Ok(rows) = latest(db) else { return windows };
    for row in rows.iter().filter(|row| row.text("provider") == Some("claude") && row.text("kind") == Some("window")) {
        let Some(reading) = row.reading() else { continue };
        if reading.used.0 >= FULL_PCT && reading.resets_at.is_some_and(|at| at.0 as u64 * 1000 > now_ms) {
            windows.limits.push(LimitEvent { account: reading.account.clone(), window: reading.window, resets_at: reading.resets_at, at: reading.read_at });
        }
        windows.readings.push(reading);
    }
    windows
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
            other => weekly.cells[COLUMNS.iter().position(|name| *name == other).unwrap_or_default()].clone(),
        })
        .collect();
    Row { cells, id: 0 }
}

impl Row {
    fn reading(&self) -> Option<Reading> {
        let seconds = |text: Option<&str>| text.and_then(|text| Timestamp::parse(text).ok()).map(|at| at.unix_millis().div_euclid(1000));
        let (window, read_at, used) = (self.text("name").and_then(meter::window)?, seconds(self.text("ts"))?, self.number("used_pct")?);
        let source = match self.text("source").and_then(|source| source.split_whitespace().next()) {
            Some("header") => Via::Header,
            Some("stream") => Via::Stream,
            Some("web") => Via::Web,
            _ => Via::Cli,
        };
        let account = Label(self.text("account").unwrap_or_default().to_owned());
        Some(Reading { row: RowId(self.id), account, window, used: Percent(used), resets_at: seconds(self.text("resets_at")).map(Unix), read_at: Unix(read_at), source })
    }

    pub(super) fn text(&self, column: &str) -> Option<&str> {
        match &self.cells[COLUMNS.iter().position(|name| *name == column)?] {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(super) fn number(&self, column: &str) -> Option<f64> {
        match &self.cells[COLUMNS.iter().position(|name| *name == column)?] {
            Value::Real(number) => Some(*number),
            Value::Integer(number) => Some(*number as f64),
            _ => None,
        }
    }

    pub(super) fn values(&self) -> impl Iterator<Item = (&'static str, &Value)> {
        COLUMNS.iter().copied().zip(&self.cells)
    }

    pub(super) fn value(&self, column: &str) -> Option<&Value> {
        self.cells.get(COLUMNS.iter().position(|name| *name == column)?)
    }

    pub(super) fn set(&mut self, column: &str, value: Value) {
        if let Some(index) = COLUMNS.iter().position(|name| *name == column) {
            self.cells[index] = value;
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
