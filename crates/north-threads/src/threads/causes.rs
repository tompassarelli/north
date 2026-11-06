//! `north cause record`: the cause a boundary of the harness (the lander, dispatch, the launch governor, capacity, the
//! fleet) names for the session it touched. The ledger ingest prefers these rows over report phrases
//! (north#706, docs/rationale/attribution.md). The table is created on first write.
use std::io::{Read, Write};

use rusqlite::params;

use super::attribution::Cause;
use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};

pub const VERBS: [&str; 1] = ["cause"];

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS boundary_causes(session TEXT NOT NULL CHECK(length(session) > 0), source TEXT NOT NULL CHECK(length(source) > 0), cause TEXT NOT NULL CHECK(length(cause) > 0), reason TEXT NOT NULL DEFAULT '', at TEXT NOT NULL) STRICT;
CREATE INDEX IF NOT EXISTS boundary_causes_session ON boundary_causes(session, at);";

const USAGE: &str = "usage: north cause record --session NAME --source BOUNDARY --cause worker|tom|<harness class> [--reason TEXT]";

pub fn run(argv: &[String], context: &Context, _stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv.get(1).map(String::as_str) {
        Some("record") => record(&argv[2..], context),
        _ => Err(Failure::Usage(USAGE.into())),
    };
    report(&argv[0], result, out, err)
}

fn required(args: &Args, flag: &str) -> Result<String, Failure> {
    args.get(flag).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| Failure::Usage(format!("{flag} is required; {USAGE}")))
}

fn record(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--session", "--source", "--cause", "--reason"], &[])?;
    let (session, source, cause) = (required(&args, "--session")?, required(&args, "--source")?, required(&args, "--cause")?);
    if Cause::parse(&cause).is_none() {
        return Err(Failure::Usage(format!("--cause {cause} names neither worker, tom nor a harness class; {USAGE}")));
    }
    if context.replica {
        return Err(Failure::Refused("this ledger is a replica; causes are written on the ledger host".into()));
    }
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    tx.execute("INSERT INTO boundary_causes(session, source, cause, reason, at) VALUES (?1, ?2, ?3, ?4, ?5)", params![session, source, cause, args.get("--reason").unwrap_or_default(), context.now.iso()])?;
    tx.commit()?;
    Ok(format!("cause {cause} for {session}: recorded by {source}\n"))
}
