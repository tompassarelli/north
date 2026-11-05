//! What every NorthThreads verb shares: the ledger, argparse-shaped arguments, refusals, events
//! in the JSON form the ledger has always held, and the outside effects.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OpenFlags, ToSql, TransactionBehavior, params};

use crate::foundation::clock::Timestamp;
use crate::platform::pyjson::Value;
use crate::threads::catalog::CATALOG;
use crate::threads::claims::issues::Issues;


static NATIVE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\b(?:{})#\d+\b", CATALOG.native.join("|"))).expect("native ref pattern"));

/// What a verb touches outside the ledger.
pub trait Outside: Issues {
    /// Refuses text holding a secret.
    fn scan(&self, text: &str) -> Result<(), String>;
    /// Runs gh with these arguments and returns its stdout.
    fn gh(&self, args: &[String]) -> Result<String, String>;
    /// Who owns a session alive now, here or as relayed by the caller; `None` when no fleet could be read.
    fn alive_owners(&self) -> Option<BTreeSet<String>>;
    /// Each issue an open gate decides, with the gate's label.
    fn gated_issues(&self) -> BTreeMap<String, String>;
    /// Each service's dollars per week from the operator's costs.toml.
    fn prices(&self) -> Result<BTreeMap<String, f64>, String>;
    /// The operator's costs.toml the prices come from.
    fn costs_path(&self) -> std::path::PathBuf;
}

pub struct Context<'a> {
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
    pub fn by(&self, args: &Args) -> String {
        args.get("--by").map_or_else(|| self.actor.clone(), str::to_owned)
    }

    /// gh, refused before it runs when an argument names a native item.
    pub fn gh(&self, args: &[&str]) -> Result<String, Failure> {
        if args.iter().any(|arg| NATIVE_REF.is_match(arg)) {
            return Err(Failure::Refused("a gh argument names a native item; native items never reach GitHub".into()));
        }
        Ok(self.outside.gh(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())?)
    }
}

#[derive(Debug)]
pub enum Failure {
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

pub type Outcome = Result<String, Failure>;

/// A ported verb: arguments, context, stdin, stdout and stderr in; its exit code out.
pub type Verb = fn(&[String], &Context, &mut dyn std::io::Read, &mut dyn Write, &mut dyn Write) -> i32;

/// Prints a verb's result to `out` or its refusal to `err`; returns the exit code.
pub fn report(verb: &str, result: Outcome, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
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
pub struct Args {
    pub positional: Vec<String>,
    pub options: BTreeMap<String, String>,
}

impl Args {
    pub fn parse(argv: &[String], valued: &[&str], switches: &[&str]) -> Result<Self, Failure> {
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

    pub fn get(&self, flag: &str) -> Option<&str> {
        self.options.get(flag).map(String::as_str)
    }

    pub fn has(&self, flag: &str) -> bool {
        self.options.contains_key(flag)
    }
}

pub fn open(path: &Path, write: bool) -> Result<Connection, Failure> {
    let access = if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let failed = |error: rusqlite::Error| Failure::Refused(format!("ledger {}: {error}", path.display()));
    let db = Connection::open_with_flags(path, access | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    crate::threads::hold::wait_when_busy(&db).map_err(failed)?;
    Ok(db)
}

pub fn immediate(db: &mut Connection) -> Result<rusqlite::Transaction<'_>, Failure> {
    Ok(db.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

/// A ref as NorthThreads normalizes it, refused for a native item this caller may not read.
pub fn item_ref(text: &str, context: &Context) -> Result<(String, u64), Failure> {
    let (repo, number) = CATALOG.parse_ref(text)?;
    if let Some(reason) = context.no_native.filter(|_| CATALOG.is_native(&repo)) {
        return Err(Failure::Refused(format!("native items are not readable here: {reason}")));
    }
    Ok((repo, number))
}

pub fn is_native_ref(item: &str) -> bool {
    NATIVE_REF.find(item).is_some_and(|found| found.start() == 0 && found.end() == item.len())
}

/// One ledger event, its data as Python's `json.dumps` writes it.
pub fn event(db: &Connection, item: Option<&str>, kind: &str, actor: &str, data: Value, now: Timestamp) -> rusqlite::Result<usize> {
    event_as(db, item, kind, &actor, &"", data, now)
}

/// One ledger event by an actor of a tier, as a worker run records it.
pub fn event_as(db: &Connection, item: Option<&str>, kind: &str, actor: &dyn ToSql, tier: &dyn ToSql, data: Value, now: Timestamp) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO events(ts, item, kind, actor, tier, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![now.iso(), item, kind, actor, tier, crate::platform::pyjson::dumps(&data)])
}

pub fn integer(value: i64) -> Value {
    Value::Number { raw: value.to_string(), output: value.to_string() }
}

pub fn text_or_null(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::from)
}

/// Columns padded to their widest cell, two spaces apart, trailing space trimmed.
pub fn table(rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    let line = |row: &Vec<String>| row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_owned();
    rows.iter().map(|row| format!("{}\n", line(row))).collect()
}

#[cfg(test)]
mod tests {
    use super::{Context, Failure};
    use crate::foundation::clock::Timestamp;
    use crate::threads::catalog::CATALOG;
    use crate::threads::fake::Recorded;

    #[test]
    fn gh_refuses_every_argv_naming_a_generated_native_ref_before_it_runs() {
        let outside = Recorded::default();
        let context = Context { database: std::path::Path::new("/dev/null"), actor: "tester".into(), no_native: None, outside: &outside, now: Timestamp::from_unix_seconds(0), replica: false };
        let mut state = 7_u64;
        let mut next = |bound: u64| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % bound
        };
        let mut refused = 0;
        for index in 0..1000 {
            let reference = format!("{}#{}", CATALOG.native[next(CATALOG.native.len() as u64) as usize], 1000 + index);
            let argv = match next(4) {
                0 => vec!["issue".to_owned(), "comment".into(), "1".into(), "--body".into(), format!("see {reference}")],
                1 => vec!["issue".to_owned(), "create".into(), "--title".into(), format!("{reference} title")],
                2 => vec!["api".to_owned(), "graphql".into(), "-f".into(), format!("query={reference}")],
                _ => vec!["pr".to_owned(), "edit".into(), "2".into(), format!("--body=x ({reference})")],
            };
            let words: Vec<&str> = argv.iter().map(String::as_str).collect();
            if matches!(context.gh(&words), Err(Failure::Refused(_))) {
                refused += 1;
            }
        }
        assert_eq!((refused, outside.calls.borrow().len()), (1000, 0));
    }
}
