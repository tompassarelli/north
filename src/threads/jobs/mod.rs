//! `north threads job-run`, `job-runs` and `queue`: scheduled job runs, and the queue of jobs for
//! machines that may be asleep, in the ledger bin/threads has migrated.
pub(crate) mod outside;
mod queue;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

use crate::foundation::clock::Timestamp;
use crate::pyjson::Value;
use crate::threads::catalog::CATALOG;

pub(crate) const VERBS: [&str; 3] = ["job-run", "job-runs", "queue"];
/// Set by bin/threads once it has migrated THREADS_DB, which it owns until its migrations move here.
pub(crate) const SCHEMA_READY: &str = "THREADS_SCHEMA_READY";
const BUSY: Duration = Duration::from_secs(60);

static NATIVE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\b(?:{})#\d+\b", CATALOG.native.join("|"))).expect("native ref pattern"));

/// What a queue result touches outside the ledger.
pub(crate) trait Outside {
    /// Refuses text holding a secret.
    fn scan(&self, text: &str) -> Result<(), String>;
    fn comment(&self, repo: &str, number: u64, body: &str) -> Result<(), String>;
}

pub(crate) struct Context<'a> {
    pub database: &'a Path,
    pub actor: String,
    /// Why native items are hidden from this caller, when they are.
    pub no_native: Option<&'static str>,
    pub outside: &'a dyn Outside,
    pub now: Timestamp,
}

pub(crate) enum Failure {
    Usage(String),
    Refused(String),
}

impl From<String> for Failure {
    fn from(why: String) -> Self {
        Self::Refused(why)
    }
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Refused(error.to_string())
    }
}

type Outcome = Result<String, Failure>;

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub(crate) fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv[0].as_str() {
        "job-run" => job_run(&argv[1..], context),
        "job-runs" => job_runs(&argv[1..], context),
        _ => queue::run(&argv[1..], context, stdin),
    };
    match result {
        Ok(text) => i32::from(write!(out, "{text}").is_err()),
        Err(Failure::Usage(why)) => {
            let _ = writeln!(err, "NorthThreads: {}: {why}", argv[0]);
            2
        }
        Err(Failure::Refused(why)) => {
            let _ = writeln!(err, "NorthThreads: {why}");
            1
        }
    }
}

/// Options and positional words, the way argparse reads them.
pub(crate) struct Args {
    pub positional: Vec<String>,
    pub options: BTreeMap<String, String>,
}

impl Args {
    pub(crate) fn parse(argv: &[String], valued: &[&str], switches: &[&str]) -> Result<Self, Failure> {
        let mut parsed = Self { positional: Vec::new(), options: BTreeMap::new() };
        let mut rest = argv.iter();
        while let Some(arg) = rest.next() {
            let (flag, inline) = arg.split_once('=').map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value.to_owned())));
            if valued.contains(&flag) || flag == "--by" {
                let value = inline.or_else(|| rest.next().cloned()).ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))?;
                parsed.options.insert(flag.to_owned(), value);
            } else if switches.contains(&arg.as_str()) {
                parsed.options.insert(arg.clone(), String::new());
            } else if arg.starts_with('-') && arg.len() > 1 {
                return Err(Failure::Usage(format!("unknown option {arg}")));
            } else {
                parsed.positional.push(arg.clone());
            }
        }
        Ok(parsed)
    }

    pub(crate) fn get(&self, flag: &str) -> Option<&str> {
        self.options.get(flag).map(String::as_str)
    }

    pub(crate) fn has(&self, flag: &str) -> bool {
        self.options.contains_key(flag)
    }
}

pub(crate) fn open(path: &Path, write: bool) -> Result<Connection, Failure> {
    let access = if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let failed = |error: rusqlite::Error| Failure::Refused(format!("ledger {}: {error}", path.display()));
    let db = Connection::open_with_flags(path, access | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    db.busy_timeout(BUSY).map_err(failed)?;
    Ok(db)
}

pub(crate) fn immediate(db: &mut Connection) -> Result<rusqlite::Transaction<'_>, Failure> {
    Ok(db.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

/// A ref as bin/threads normalizes it, refused for a native item this caller may not read.
pub(crate) fn item_ref(text: &str, context: &Context) -> Result<(String, u64), Failure> {
    let (repo, number) = CATALOG.parse_ref(text)?;
    if let Some(reason) = context.no_native.filter(|_| CATALOG.is_native(&repo)) {
        return Err(Failure::Refused(format!("native items are not readable here: {reason}")));
    }
    Ok((repo, number))
}

pub(crate) fn is_native_ref(item: &str) -> bool {
    NATIVE_REF.find(item).is_some_and(|found| found.start() == 0 && found.end() == item.len())
}

pub(crate) fn names_native(text: &str) -> bool {
    NATIVE_REF.is_match(text)
}

/// One ledger event, its data as Python's `json.dumps` writes it.
pub(crate) fn event(db: &Connection, item: Option<&str>, kind: &str, actor: &str, data: Value, now: Timestamp) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO events(ts, item, kind, actor, tier, data) VALUES (?1, ?2, ?3, ?4, '', ?5)", params![now.iso(), item, kind, actor, crate::pyjson::dumps(&data)])
}

pub(crate) fn integer(value: i64) -> Value {
    Value::Number { raw: value.to_string(), output: value.to_string() }
}

pub(crate) fn text_or_null(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::from)
}

/// Columns padded to their widest cell, two spaces apart, trailing space trimmed.
pub(crate) fn table(rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    let line = |row: &Vec<String>| row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_owned();
    rows.iter().map(|row| format!("{}\n", line(row))).collect()
}

fn job_run(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--host", "--started", "--finished", "--rc"], &[])?;
    let [name] = args.positional.as_slice() else { return Err(Failure::Usage("usage: threads job-run NAME --host HOST --started TIME [--finished TIME --rc N]".into())) };
    let (Some(host), Some(started)) = (args.get("--host"), args.get("--started")) else {
        return Err(Failure::Usage("--host and --started are required".into()));
    };
    let rc = args.get("--rc").map(|rc| rc.parse::<i64>().map_err(|_| Failure::Usage(format!("--rc {rc} is not an integer")))).transpose()?;
    let finished = args.get("--finished");
    if finished.is_none() != rc.is_none() {
        return Err(Failure::Refused("job-run: --finished and --rc go together".into()));
    }
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute(
        "INSERT INTO job_runs(name, host, started, finished, rc) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(name, host, started) \
         DO UPDATE SET finished = excluded.finished, rc = excluded.rc WHERE excluded.finished IS NOT NULL",
        params![name, host, started, finished, rc],
    )?;
    tx.commit()?;
    Ok(String::new())
}

const JOB_RUNS_JSON: &str = "SELECT name, host, min(started), max(started), \
    (SELECT max(finished) FROM job_runs j WHERE j.name = r.name AND j.host = r.host AND j.rc = 0), \
    (SELECT rc FROM job_runs j WHERE j.name = r.name AND j.host = r.host ORDER BY started DESC LIMIT 1), \
    (SELECT max(started) FROM job_runs j WHERE j.name = r.name AND j.host = r.host AND j.finished IS NULL) \
    FROM job_runs r GROUP BY name, host ORDER BY host, name";
const JOB_RUNS: &str = "SELECT name, host, count(*), sum(rc != 0), sum(finished IS NULL), max(started), \
    (SELECT rc FROM job_runs j WHERE j.name = r.name AND j.host = r.host ORDER BY started DESC LIMIT 1) \
    FROM job_runs r GROUP BY name, host ORDER BY host, name";

fn job_runs(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &["--json"])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads job-runs [--json]".into()));
    }
    let db = open(context.database, false)?;
    if args.has("--json") {
        let mut statement = db.prepare(JOB_RUNS_JSON)?;
        let rows = statement.query_map([], |row| {
            Ok(Value::object([
                ("name", Value::from(row.get::<_, String>(0)?)),
                ("host", Value::from(row.get::<_, String>(1)?)),
                ("first_started", text_or_null(row.get(2)?)),
                ("last_started", text_or_null(row.get(3)?)),
                ("last_ok", text_or_null(row.get(4)?)),
                ("last_rc", row.get::<_, Option<i64>>(5)?.map_or(Value::Null, integer)),
                ("open_since", text_or_null(row.get(6)?)),
            ]))
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(format!("{}\n", crate::pyjson::dumps(&Value::Array(rows))));
    }
    let mut statement = db.prepare(JOB_RUNS)?;
    let rows = statement.query_map([], |row| {
        let count = |index: usize| -> rusqlite::Result<String> { Ok(row.get::<_, Option<i64>>(index)?.unwrap_or(0).to_string()) };
        Ok(vec![row.get(0)?, row.get(1)?, count(2)?, count(3)?, count(4)?, row.get(5)?, row.get::<_, Option<i64>>(6)?.map_or_else(|| "-".to_owned(), |rc| rc.to_string())])
    })?;
    let rows = rows.collect::<rusqlite::Result<Vec<Vec<String>>>>()?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    let header = ["name", "host", "runs", "failed", "open", "last_started", "last_rc"].map(str::to_owned).to_vec();
    Ok(table(&[vec![header], rows].concat()))
}
