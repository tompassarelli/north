mod read;
mod resolve;
mod validate;
mod write;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{GraphError, Result};

pub use read::{Live, Told};
pub use resolve::Terms;
pub use validate::Report;
pub use write::{Action, ActionKind, Receipt};

const SCHEMA_VERSION: i64 = 2;
const PATIENCE: Duration = Duration::from_secs(30);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS terms (id INTEGER PRIMARY KEY, key BLOB NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS occurrences (
    seq INTEGER PRIMARY KEY,
    coord INTEGER NOT NULL UNIQUE,
    action INTEGER NOT NULL,
    prop INTEGER NOT NULL,
    writer INTEGER NOT NULL,
    live INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS occurrences_prop ON occurrences (prop, action);
CREATE INDEX IF NOT EXISTS occurrences_live ON occurrences (prop, seq) WHERE live = 1;
CREATE TABLE IF NOT EXISTS withdrawals (retraction INTEGER PRIMARY KEY, assertion INTEGER NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS live_props (
    prop INTEGER PRIMARY KEY,
    t1 INTEGER NOT NULL,
    t2 INTEGER NOT NULL,
    t3 INTEGER NOT NULL,
    n INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS live_props_t1 ON live_props (t1, t2, t3);
CREATE INDEX IF NOT EXISTS live_props_t2 ON live_props (t2, t3, t1);
CREATE INDEX IF NOT EXISTS live_props_t3 ON live_props (t3, t1, t2);
CREATE TABLE IF NOT EXISTS quoted (
    id INTEGER PRIMARY KEY,
    t1 INTEGER NOT NULL,
    t2 INTEGER NOT NULL,
    t3 INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS quoted_t1 ON quoted (t1, t2, t3);
CREATE INDEX IF NOT EXISTS quoted_t2 ON quoted (t2, t3, t1);
CREATE INDEX IF NOT EXISTS quoted_t3 ON quoted (t3, t1, t2);
CREATE VIEW IF NOT EXISTS assertions AS
    SELECT o.writer AS writer, p.t1 AS t1, p.t2 AS t2, p.t3 AS t3 FROM occurrences o JOIN live_props p ON p.prop = o.prop WHERE o.live = 1;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
";

pub struct Graph {
    connection: Connection,
}

pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NORTH_GRAPH_DB").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north/graph.db")
}

impl Graph {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|why| GraphError::Corrupt(format!("{}: {why}", parent.display())))?;
        }
        Self::prepare(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> Result<Self> {
        connection.busy_timeout(PATIENCE)?;
        connection.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err(GraphError::Corrupt(format!("graph schema version {version}, this build reads {SCHEMA_VERSION}")));
        }
        connection.execute_batch(SCHEMA)?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { connection })
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }
}
