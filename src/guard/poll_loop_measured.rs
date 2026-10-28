use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, params};

use crate::shell;

const SIZING_RUNS: i64 = 20;
const MINIMUM_RUNS: usize = 3;

pub(super) struct MeasuredDuration {
    pub shape: String,
    pub seconds: f64,
    pub runs: usize,
}

pub(super) fn database() -> PathBuf {
    if let Some(path) = std::env::var_os("AGENT_CAPACITY_DB").filter(|path| !path.is_empty()) {
        return path.into();
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("agents/machine-capacity/capacity.db")
}

pub(super) fn waited_on(source: &str, db: &Path) -> Option<MeasuredDuration> {
    let words = shell::words(&shell::strip_heredocs(source))?;
    let connection =
        Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    connection.busy_timeout(Duration::from_millis(200)).ok()?;
    let mut shapes = connection.prepare("SELECT DISTINCT shape FROM capacity_runs WHERE shape LIKE '% %'").ok()?;
    let shape = shapes
        .query_map([], |row| row.get::<_, String>(0))
        .ok()?
        .filter_map(Result::ok)
        .filter(|shape| contains_run(&words, shape))
        .max_by_key(|shape| shape.split(' ').count())?;
    let mut recent = connection
        .prepare(
            "SELECT wallSeconds FROM capacity_runs WHERE shape = ?1 AND exitCode = 0 AND wallSeconds > 0 \
             ORDER BY at DESC LIMIT ?2",
        )
        .ok()?;
    let mut seconds: Vec<f64> =
        recent.query_map(params![shape, SIZING_RUNS], |row| row.get(0)).ok()?.filter_map(Result::ok).collect();
    if seconds.len() < MINIMUM_RUNS {
        return None;
    }
    seconds.sort_by(f64::total_cmp);
    let rank = (seconds.len() * 3).div_ceil(4) - 1;
    Some(MeasuredDuration { shape, seconds: seconds[rank], runs: seconds.len() })
}

fn contains_run(words: &[String], shape: &str) -> bool {
    let wanted: Vec<&str> = shape.split(' ').collect();
    words.windows(wanted.len()).any(|window| window.iter().map(String::as_str).eq(wanted.iter().copied()))
}
