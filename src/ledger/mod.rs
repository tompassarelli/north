use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{Connection, Row, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::turns::{RunRecord, RunSink};

mod books;
mod briefs;
pub(crate) mod closed;
mod closed_source;
mod gates;
pub mod planner;
pub(crate) mod pricing;
mod report;
mod verdict;

pub use closed::{ClosedReport, Figures};
#[cfg(test)]
pub(crate) use closed::{SpendRun, summarize};
pub use closed_source::{PriorReport, arm_runs, closed_report, closed_report_and_previous, prior_reports, window_start_ms};
pub use report::{print_runs, print_summary};
pub use verdict::{check, landed};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs(
  run_id INTEGER PRIMARY KEY,
  session TEXT NOT NULL,
  task_ref TEXT,
  provider TEXT NOT NULL,
  model TEXT,
  effort TEXT,
  account TEXT,
  prompt_sha TEXT,
  prompt TEXT,
  system_prompt_sha TEXT,
  deepen INTEGER NOT NULL DEFAULT 0 CHECK(deepen IN (0,1)),
  started_ms INTEGER NOT NULL,
  ended_ms INTEGER NOT NULL,
  wall_ms INTEGER NOT NULL,
  turns INTEGER NOT NULL,
  tokens_in INTEGER NOT NULL,
  tokens_out INTEGER NOT NULL,
  tokens_cache_read INTEGER NOT NULL,
  tokens_cache_write INTEGER NOT NULL,
  exit TEXT NOT NULL CHECK(exit IN ('completed','interrupted','failed','killed'))
) STRICT;
CREATE TABLE IF NOT EXISTS tasks(
  task_ref TEXT PRIMARY KEY,
  category TEXT,
  sessions TEXT NOT NULL DEFAULT '[]',
  wall_ms INTEGER,
  outcome TEXT CHECK(outcome IN ('landed','abandoned','not done')),
  landed_sha TEXT,
  landed_ms INTEGER,
  review TEXT,
  corrections INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS checks(
  check_id INTEGER PRIMARY KEY,
  task_ref TEXT NOT NULL,
  command TEXT NOT NULL,
  cwd TEXT NOT NULL,
  head_sha TEXT,
  exit INTEGER NOT NULL,
  ran_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS gates(
  gate_id INTEGER PRIMARY KEY,
  session TEXT NOT NULL,
  question TEXT NOT NULL,
  options TEXT NOT NULL,
  recommend INTEGER NOT NULL,
  default_option INTEGER NOT NULL,
  default_after_ms INTEGER NOT NULL,
  opened_ms INTEGER NOT NULL,
  answer TEXT,
  answered_by TEXT,
  closed_ms INTEGER,
  verification INTEGER NOT NULL DEFAULT 0,
  issue TEXT
) STRICT;
CREATE INDEX IF NOT EXISTS runs_by_session ON runs(session);
CREATE TABLE IF NOT EXISTS events(
  event_id INTEGER PRIMARY KEY,
  session TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload TEXT NOT NULL,
  recorded_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS receipts(
  session TEXT NOT NULL,
  receipt TEXT NOT NULL,
  claimed_ms INTEGER NOT NULL,
  PRIMARY KEY(session, receipt)
) STRICT;
";

const SCHEMA_VERSION: i64 = 2;

const CHECKPOINT_EVERY: Duration = Duration::from_secs(60);

const LANDING_CHECK: &str = "safe-push landing";

const LINK_RUNS: &str = "UPDATE runs SET task_ref = (SELECT t.task_ref FROM tasks t WHERE t.task_ref = runs.session OR EXISTS (SELECT 1 FROM json_each(t.sessions) WHERE value = runs.session) ORDER BY t.task_ref LIMIT 1) \
     WHERE task_ref IS NULL AND EXISTS (SELECT 1 FROM tasks t WHERE t.task_ref = runs.session OR EXISTS (SELECT 1 FROM json_each(t.sessions) WHERE value = runs.session))";

const LINK_RUNS_OF_SESSION: &str = "UPDATE runs SET task_ref = (SELECT t.task_ref FROM tasks t WHERE t.task_ref = runs.session OR EXISTS (SELECT 1 FROM json_each(t.sessions) WHERE value = runs.session) ORDER BY t.task_ref LIMIT 1) \
     WHERE task_ref IS NULL AND session = ?1 AND EXISTS (SELECT 1 FROM tasks t WHERE t.task_ref = runs.session OR EXISTS (SELECT 1 FROM json_each(t.sessions) WHERE value = runs.session))";

/// How long a statement meeting another writer's lock retries, through SQLite's backoff, before it fails.
const LOCK_WAIT: Duration = Duration::from_secs(30);

const RUN_COLUMNS: &str = "run_id,session,task_ref,provider,model,effort,account,prompt_sha,system_prompt_sha,deepen,started_ms,ended_ms,wall_ms,turns,\
     tokens_in,tokens_out,tokens_cache_read,tokens_cache_write,exit";

pub struct Ledger {
    conn: Mutex<Connection>,
    checkpointed: Mutex<Option<Instant>>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Run {
    pub run_id: i64,
    pub session: String,
    pub task_ref: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub account: Option<String>,
    pub prompt_sha: Option<String>,
    pub system_prompt_sha: Option<String>,
    pub deepen: bool,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub wall_ms: i64,
    pub turns: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub exit: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Task {
    pub task_ref: String,
    pub sessions: serde_json::Value,
    pub wall_ms: Option<i64>,
    pub outcome: Option<String>,
    pub landed_sha: Option<String>,
    pub review: Option<String>,
    pub checks: i64,
    pub last_check_exit: Option<i64>,
    pub last_check_sha: Option<String>,
}

pub struct CheckRun<'a> {
    pub task_ref: &'a str,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub head_sha: Option<&'a str>,
    pub exit: i32,
    pub ran_ms: i64,
}

impl Ledger {
    pub fn record_native_solo(&self, session: &str, reason: &str, command: &str) -> NorthResult<()> {
        let payload = serde_json::json!({"reason": reason, "command": command});
        self.write(|conn| {
            conn.execute(
                "INSERT INTO events(session, kind, payload, recorded_ms) VALUES(?1, 'native-solo', ?2, CAST(unixepoch('subsec') * 1000 AS INTEGER))",
                params![session, payload.to_string()],
            ).map(drop)
        })
    }

    pub fn open_default() -> NorthResult<Self> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| NorthError::Configuration("HOME is not set".into()))?;
        Self::open(&PathBuf::from(home).join(".local/state/north/ledger.db"))
    }

    pub fn open(path: &Path) -> NorthResult<Self> {
        Self::open_within(path, LOCK_WAIT)
    }

    fn open_within(path: &Path, lock_wait: Duration) -> NorthResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(sql)?;
        conn.busy_timeout(lock_wait).map_err(sql)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(sql)?;
        if schema_version(&conn).map_err(sql)? < SCHEMA_VERSION {
            migrate(&conn).map_err(sql)?;
        }
        Ok(Self { conn: Mutex::new(conn), checkpointed: Mutex::new(None) })
    }

    pub fn runs(&self) -> NorthResult<Vec<Run>> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!("SELECT {RUN_COLUMNS} FROM runs ORDER BY run_id"))?;
            statement.query_map([], run_from_row)?.collect()
        })
    }

    pub fn tasks(&self) -> NorthResult<Vec<Task>> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT t.task_ref, t.sessions, t.wall_ms, t.outcome, t.landed_sha, t.review,
                   (SELECT count(*) FROM checks c WHERE c.task_ref = t.task_ref),
                   (SELECT c.exit FROM checks c WHERE c.task_ref = t.task_ref ORDER BY c.check_id DESC LIMIT 1),
                   (SELECT c.head_sha FROM checks c WHERE c.task_ref = t.task_ref ORDER BY c.check_id DESC LIMIT 1)
                 FROM tasks t ORDER BY t.task_ref",
            )?;
            statement.query_map([], task_from_row)?.collect()
        })
    }

    pub fn landing_links(&self, since_ms: i64) -> NorthResult<(i64, i64)> {
        self.with(|conn| {
            conn.query_row(
                "SELECT count(*), coalesce(sum(EXISTS (SELECT 1 FROM runs r WHERE r.task_ref = t.task_ref)), 0) FROM tasks t WHERE t.outcome = 'landed' AND t.landed_ms >= ?1",
                [since_ms],
                |row| Ok((row.get(1)?, row.get(0)?)),
            )
        })
    }

    pub fn record_check(&self, check: &CheckRun) -> NorthResult<()> {
        self.write(|conn| {
            if conn.execute("INSERT OR IGNORE INTO tasks(task_ref) VALUES(?1)", [check.task_ref])? == 1 {
                conn.execute_batch(LINK_RUNS)?;
            }
            conn.execute(
                "INSERT INTO checks(task_ref, command, cwd, head_sha, exit, ran_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![check.task_ref, check.command, check.cwd.display().to_string(), check.head_sha, check.exit, check.ran_ms],
            )
            .map(drop)
        })
    }

    fn write<T>(&self, work: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> NorthResult<T> {
        let conn = self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let transaction = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate).map_err(sql)?;
        let output = work(&transaction).map_err(sql)?;
        transaction.commit().map_err(sql)?;
        self.checkpoint_when_due(&conn);
        Ok(output)
    }

    fn checkpoint_when_due(&self, conn: &Connection) {
        let mut last = self.checkpointed.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| at.elapsed() < CHECKPOINT_EVERY) {
            return;
        }
        *last = Some(Instant::now());
        conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(())).ok();
    }

    fn with<T>(&self, work: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> NorthResult<T> {
        work(&self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())).map_err(sql)
    }
}

impl ReceiptBook for Ledger {
    fn claim_receipt(&self, session: &str, key: &str, claimed_ms: u64) -> NorthResult<bool> {
        self.write(|conn| {
            conn.execute("INSERT OR IGNORE INTO receipts(session, receipt, claimed_ms) VALUES(?1, ?2, ?3)", params![session, key, millis(claimed_ms)])
                .map(|inserted| inserted == 1)
        })
    }
}

impl RunSink for Ledger {
    fn record_run(&self, run: &RunRecord) -> NorthResult<u64> {
        let turn = &run.turn;
        self.write(|conn| {
            conn.execute(
                "INSERT INTO runs(session, task_ref, provider, model, effort, account, prompt_sha, prompt, system_prompt_sha, deepen,
                   started_ms, ended_ms, wall_ms, turns, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write, exit)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                params![
                    run.session,
                    run.task_ref,
                    run.provider,
                    turn.model,
                    turn.effort,
                    run.account,
                    turn.prompt.as_deref().map(sha),
                    turn.prompt,
                    run.system_prompt_sha,
                    run.deepen,
                    millis(turn.started_ms),
                    millis(turn.ended_ms),
                    millis(turn.wall_ms),
                    millis(turn.turns),
                    millis(turn.tokens.input),
                    millis(turn.tokens.output),
                    millis(turn.tokens.cache_read),
                    millis(turn.tokens.cache_write),
                    turn.exit.label(),
                ],
            )?;
            conn.execute(LINK_RUNS_OF_SESSION, [&run.session])?;
            briefs::link_by_brief(conn, Some(&run.session))?;
            conn.query_row("SELECT count(*) FROM runs WHERE session = ?1", [&run.session], |row| row.get::<_, i64>(0)).map(|count| u64::try_from(count).unwrap_or(0))
        })
    }

    fn join_task(&self, task_ref: &str, session: &str) -> NorthResult<()> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO tasks(task_ref, sessions) VALUES(?1, json_array(?2))
                 ON CONFLICT(task_ref) DO UPDATE SET sessions = json_insert(tasks.sessions, '$[#]', ?2)
                 WHERE NOT EXISTS (SELECT 1 FROM json_each(tasks.sessions) WHERE value = ?2)",
                params![task_ref, session],
            )?;
            conn.execute(LINK_RUNS_OF_SESSION, [session]).map(drop)
        })
    }
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        run_id: row.get(0)?,
        session: row.get(1)?,
        task_ref: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        effort: row.get(5)?,
        account: row.get(6)?,
        prompt_sha: row.get(7)?,
        system_prompt_sha: row.get(8)?,
        deepen: row.get(9)?,
        started_ms: row.get(10)?,
        ended_ms: row.get(11)?,
        wall_ms: row.get(12)?,
        turns: row.get(13)?,
        tokens_in: row.get(14)?,
        tokens_out: row.get(15)?,
        tokens_cache_read: row.get(16)?,
        tokens_cache_write: row.get(17)?,
        exit: row.get(18)?,
    })
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    let sessions: String = row.get(1)?;
    Ok(Task {
        task_ref: row.get(0)?,
        sessions: serde_json::from_str(&sessions).unwrap_or_default(),
        wall_ms: row.get(2)?,
        outcome: row.get(3)?,
        landed_sha: row.get(4)?,
        review: row.get(5)?,
        checks: row.get(6)?,
        last_check_exit: row.get(7)?,
        last_check_sha: row.get(8)?,
    })
}

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if schema_version(&transaction)? < SCHEMA_VERSION {
        transaction.execute_batch(SCHEMA)?;
        add_missing_column(&transaction, "gates", "verification", "INTEGER NOT NULL DEFAULT 0")?;
        add_missing_column(&transaction, "gates", "issue", "TEXT")?;
        gates::allow_session_answerers(&transaction)?;
        add_missing_column(&transaction, "tasks", "landed_ms", "INTEGER")?;
        transaction.execute_batch(LINK_RUNS)?;
        briefs::link_by_brief(&transaction, None)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    transaction.commit()
}

fn add_missing_column(conn: &Connection, table: &str, column: &str, kind: &str) -> rusqlite::Result<()> {
    let present: bool = conn.query_row(&format!("SELECT count(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"), [column], |row| row.get(0))?;
    if !present {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
    }
    Ok(())
}

fn millis(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn sql(error: rusqlite::Error) -> NorthError {
    NorthError::State(format!("ledger: {error}"))
}

#[cfg(test)]
mod tests;
