use rusqlite::{Connection, params_from_iter};
use serde_json::Value;
use std::path::PathBuf;

const COLUMNS: [&str; 10] = [
    "ts",
    "provider",
    "account",
    "kind",
    "name",
    "used_pct",
    "window_min",
    "resets_at",
    "amount",
    "source",
];

pub(super) fn open() -> Result<(Connection, bool), String> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let explicit = std::env::var_os("THREADS_DB");
    let remote = explicit.is_none() && home.join(".config/threads/remote").exists();
    let path = explicit.map(PathBuf::from).unwrap_or_else(|| {
        home.join(if remote {
            ".local/state/threads/snapshot.db"
        } else {
            ".local/state/threads/threads.db"
        })
    });
    let db = if remote {
        let memory = Connection::open_in_memory().map_err(|e| e.to_string())?;
        if path.exists() {
            memory
                .execute(
                    "ATTACH DATABASE ? AS snapshot",
                    [path.to_string_lossy().as_ref()],
                )
                .map_err(|e| e.to_string())?;
            let exists = memory
                .query_row(
                    "SELECT count(*) FROM snapshot.sqlite_master WHERE name='usage'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            if exists > 0 {
                memory.execute_batch("CREATE TABLE usage AS SELECT * FROM snapshot.usage; DETACH DATABASE snapshot;").map_err(|e| e.to_string())?;
            }
        }
        memory
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Connection::open(path).map_err(|e| e.to_string())?
    };
    db.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    prepare(&db)?;
    Ok((db, remote))
}
pub(super) fn prepare(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS usage(ts TEXT,provider TEXT,account TEXT,kind TEXT,name TEXT,used_pct REAL,window_min INTEGER,resets_at TEXT,amount REAL,source TEXT) STRICT; CREATE INDEX IF NOT EXISTS usage_latest ON usage(provider,account,kind,name,ts); PRAGMA analysis_limit=1000; ANALYZE usage;").map_err(|e| e.to_string())
}
pub(super) fn record(db: &Connection, readings: &[Value]) -> Result<(), String> {
    let transaction = db.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut insert = transaction.prepare_cached("INSERT INTO usage(ts,provider,account,kind,name,used_pct,window_min,resets_at,amount,source) SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10 WHERE NOT EXISTS (SELECT 1 FROM usage WHERE provider IS ?2 AND account IS ?3 AND kind IS ?4 AND name IS ?5 AND ts IS ?1 AND source IS ?10)").map_err(|e| e.to_string())?;
        for row in readings {
            let values: Vec<_> = COLUMNS
                .iter()
                .map(|key| match &row[*key] {
                    Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                    Value::Number(n) => n
                        .as_i64()
                        .map(rusqlite::types::Value::Integer)
                        .unwrap_or_else(|| rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))),
                    _ => rusqlite::types::Value::Null,
                })
                .collect();
            insert
                .execute(params_from_iter(values))
                .map_err(|e| e.to_string())?;
        }
    }
    transaction.commit().map_err(|e| e.to_string())
}
