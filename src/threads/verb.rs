//! What every Rust NorthThreads verb shares: the ledger bin/threads has migrated, argparse-shaped
//! arguments, refusals, events written the way Python wrote them, and the outside effects.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use rusqlite::{Connection, OpenFlags, ToSql, TransactionBehavior, params};

use crate::foundation::clock::Timestamp;
use crate::pyjson::Value;
use crate::threads::catalog::CATALOG;
use crate::threads::claims::issues::Issues;

/// Set by bin/threads once it has migrated THREADS_DB, which it owns until its migrations move here.
pub(crate) const SCHEMA_READY: &str = "THREADS_SCHEMA_READY";
const BUSY: Duration = Duration::from_secs(60);

static NATIVE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\b(?:{})#\d+\b", CATALOG.native.join("|"))).expect("native ref pattern"));

/// What a verb touches outside the ledger.
pub(crate) trait Outside: Issues {
    /// Refuses text holding a secret.
    fn scan(&self, text: &str) -> Result<(), String>;
    /// Runs gh with these arguments and returns its stdout.
    fn gh(&self, args: &[String]) -> Result<String, String>;
    /// Who owns a session alive now, here or as relayed by the caller.
    fn alive_owners(&self) -> BTreeSet<String>;
    /// The archive tags naming an issue, with their restore commands, as `show` prints them.
    fn archives(&self, repo: &str, number: u64) -> String;
    /// Each service's dollars per week from the operator's costs.toml.
    fn prices(&self) -> Result<BTreeMap<String, f64>, String>;
}

pub(crate) struct Context<'a> {
    pub database: &'a Path,
    pub actor: String,
    /// Why native items are hidden from this caller, when they are.
    pub no_native: Option<&'static str>,
    pub outside: &'a dyn Outside,
    pub now: Timestamp,
    /// The ledger is the local snapshot of a remote one, never written.
    pub replica: bool,
}

impl Context<'_> {
    pub(crate) fn by(&self, args: &Args) -> String {
        args.get("--by").map_or_else(|| self.actor.clone(), str::to_owned)
    }

    /// gh, refused before it runs when an argument names a native item.
    pub(crate) fn gh(&self, args: &[&str]) -> Result<String, Failure> {
        if args.iter().any(|arg| NATIVE_REF.is_match(arg)) {
            return Err(Failure::Refused("a gh argument names a native item; native items never reach GitHub".into()));
        }
        Ok(self.outside.gh(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())?)
    }
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

pub(crate) type Outcome = Result<String, Failure>;

/// A ported verb: arguments, context, stdin, stdout and stderr in; its exit code out.
pub(crate) type Verb = fn(&[String], &Context, &mut dyn std::io::Read, &mut dyn Write, &mut dyn Write) -> i32;

/// Prints a verb's result to `out` or its refusal to `err`; returns the exit code.
pub(crate) fn report(verb: &str, result: Outcome, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match result {
        Ok(text) => i32::from(write!(out, "{text}").is_err()),
        Err(Failure::Usage(why)) => {
            let _ = writeln!(err, "NorthThreads: {verb}: {why}");
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
            if arg == "--" {
                parsed.positional.extend(rest.cloned());
                break;
            }
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

/// One ledger event, its data as Python's `json.dumps` writes it.
pub(crate) fn event(db: &Connection, item: Option<&str>, kind: &str, actor: &str, data: Value, now: Timestamp) -> rusqlite::Result<usize> {
    event_as(db, item, kind, &actor, &"", data, now)
}

/// One ledger event by an actor of a tier, as a worker run records it.
pub(crate) fn event_as(db: &Connection, item: Option<&str>, kind: &str, actor: &dyn ToSql, tier: &dyn ToSql, data: Value, now: Timestamp) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO events(ts, item, kind, actor, tier, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![now.iso(), item, kind, actor, tier, crate::pyjson::dumps(&data)])
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
