//! `north threads ingest`, `label-runs` and `run-ids`: north worker-ledger's JSON rows into the runs,
//! claims and usage tables, and the run ids already recorded.
pub mod brief;
mod costs;
mod labels;
mod record;
mod row;
#[cfg(test)]
mod tests;

use std::io::{Read, Write};

use rusqlite::types::Value as Sql;
use rusqlite::{OptionalExtension, params_from_iter};

use self::record::{Ingest, Tally};
use self::row::{Row, value};
use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson;

pub const VERBS: [&str; 3] = ["ingest", "label-runs", "run-ids"];
const LANDING_DAYS: i64 = 3;
/// Issue labels that name a category, first match wins, for runs whose brief is unreadable (Codex encrypts it).
pub const LABEL_CATEGORY: [(&str, &str); 8] = [
    ("theme:balance", "balance-tuning"),
    ("theme:netplay", "netcode-determinism"),
    ("needs:native-pair", "native-check"),
    ("needs:native-single", "native-check"),
    ("native:running", "native-check"),
    ("documentation", "docs-policy"),
    ("bug", "bug-known-cause"),
    ("enhancement", "feature"),
];

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv[0].as_str() {
        "ingest" => ingest(&argv[1..], context, stdin, err),
        "label-runs" => labels::label_runs(&argv[1..], context, err),
        _ => run_ids(&argv[1..], context),
    };
    report(&argv[0], result, out, err)
}

/// Claude worker rows recorded before activity columns existed keep a null active_min; leaving them out of --settled makes the ingest re-read their transcripts, and Codex and Gemini rows stay out of that re-read.
const UNMEASURED_CLAUDE: &str = "NOT (active_min IS NULL AND coalesce(tier, '') NOT LIKE 'gpt-%' AND coalesce(tier, '') NOT LIKE 'gemini%' AND coalesce(tier, '') NOT LIKE 'codex%' AND coalesce(model, '') NOT LIKE 'gpt-%' AND coalesce(model, '') NOT LIKE 'gemini%')";

/// A Claude run recorded without its model, effort or provider is left out of --settled so the ingest re-reads its transcript; Codex and Gemini rows carry no such account.
const UNIDENTIFIED_CLAUDE: &str = "(coalesce(account, '') NOT LIKE 'claude%' OR (model IS NOT NULL AND effort IS NOT NULL AND provider IS NOT NULL))";

/// Every recorded run's agent id; with --settled, only finished worker, reviewer and judge runs not still waiting to land and not a Claude run still missing its activity columns or, for Claude runs, a model, effort or provider, and with --ended each id is followed by a tab and the time its run was recorded as ending.
fn run_ids(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &["--settled", "--ended"])?;
    if !args.positional.is_empty() || (args.has("--ended") && !args.has("--settled")) {
        return Err(Failure::Usage("usage: threads run-ids [--settled [--ended]]".into()));
    }
    let db = open(context.database, false)?;
    let since = Timestamp::from_unix_seconds(context.now.unix_seconds() - LANDING_DAYS * 86400).iso();
    let settled = args.has("--settled");
    let filter = if settled { format!(" WHERE role IN (\"worker\", \"reviewer\", \"judge\") AND outcome != \"running\" AND NOT (expects_landing = 1 AND landed_at IS NULL AND ended >= ?) AND {UNMEASURED_CLAUDE} AND {UNIDENTIFIED_CLAUDE}") } else { String::new() };
    let mut query = db.prepare(&format!("SELECT agent, ended FROM runs{filter} ORDER BY agent"))?;
    let agents = query.query_map(params_from_iter(settled.then_some(since)), |row| Ok((row.get::<_, Sql>(0)?, row.get::<_, Sql>(1)?)))?;
    let mut shown = String::new();
    for agent in agents {
        let (agent, ended) = agent?;
        shown += &match agent {
            Sql::Text(text) => text,
            Sql::Integer(number) => number.to_string(),
            other => pyjson::dumps(&value(&other)),
        };
        if let (true, Sql::Text(ended)) = (args.has("--ended"), ended) {
            shown.push('\t');
            shown += &ended;
        }
        shown.push('\n');
    }
    Ok(shown)
}

/// Records every row in one transaction, then the usage readouts and the run costs they price; prints
/// what it recorded to `err`. It makes no network call, because the caller holds the exclusive ingest lock.
fn ingest(argv: &[String], context: &Context, stdin: &mut dyn Read, err: &mut dyn Write) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads ingest < rows.jsonl".into()));
    }
    let mut input = Vec::new();
    stdin.read_to_end(&mut input).map_err(|error| Failure::Refused(error.to_string()))?;
    let mut db = open(context.database, true)?;
    let (mut tally, mut readouts) = (Tally::default(), Vec::new());
    let tx = immediate(&mut db)?;
    for line in input.split(|byte| *byte == b'\n').filter(|line| !String::from_utf8_lossy(line).trim().is_empty()) {
        let row = Row::parse(line)?;
        if row.get("type").is_some_and(|kind| kind == "usage") {
            readouts.push(row);
        } else {
            Ingest { db: &tx, context, tally: &mut tally }.row(row)?;
        }
    }
    tx.commit()?;
    let recorded = if readouts.is_empty() { 0 } else { costs::record(&mut db, &readouts)? };
    let inputs_changed = recorded + tally.added + tally.samples > 0;
    if inputs_changed && db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'usage'", [], |_| Ok(())).optional()?.is_some() {
        costs::allocate(&mut db, &context.outside.prices()?)?;
    }
    let backfilled = if tally.backfilled > 0 { format!(", {} backfilled", tally.backfilled) } else { String::new() };
    let unpriced = tally.unpriced.into_iter().collect::<Vec<_>>().join(", ");
    let unpriced = if unpriced.is_empty() { unpriced } else { format!("; token weight for models without a list price: {unpriced}") };
    writeln!(err, "NorthThreads: {} runs recorded{backfilled} in {}{unpriced}", tally.added, context.database.display()).map_err(|error| Failure::Refused(error.to_string()))?;
    Ok(String::new())
}
