//! `north threads land-train record|check|red`: one row per lane a landing train decided, per check
//! run on a candidate, and per red-main interval. The tables are created here, on first write; the
//! readers in `tempo` tolerate a ledger that has none yet.
pub mod tempo;
#[cfg(test)]
mod tests;

use std::io::{Read, Write};

use rusqlite::{Connection, params};

use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};
use super::catalog::CATALOG;
use crate::foundation::clock::Timestamp;

pub const VERBS: [&str; 1] = ["land-train"];

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS landings(
  id INTEGER PRIMARY KEY,
  repo TEXT NOT NULL,
  lane TEXT NOT NULL,
  branch TEXT NOT NULL,
  tip TEXT NOT NULL,
  queued_at TEXT NOT NULL,
  candidate TEXT NOT NULL DEFAULT '',
  tree TEXT NOT NULL DEFAULT '',
  batch_size INTEGER NOT NULL CHECK(batch_size >= 0),
  bisect_depth INTEGER NOT NULL CHECK(bisect_depth >= 0),
  attempts INTEGER NOT NULL CHECK(attempts >= 0),
  actor TEXT NOT NULL DEFAULT '',
  landed_at TEXT,
  landed_sha TEXT,
  verdict TEXT NOT NULL CHECK(verdict IN ('landed','conflict','failed')),
  detail TEXT NOT NULL DEFAULT '',
  first_commit_at TEXT,
  check_started_at TEXT,
  check_finished_at TEXT,
  UNIQUE(repo, lane, tip, queued_at)
) STRICT;
CREATE INDEX IF NOT EXISTS landings_repo_queued ON landings(repo, queued_at);
CREATE TABLE IF NOT EXISTS land_checks(
  id INTEGER PRIMARY KEY,
  repo TEXT NOT NULL,
  candidate TEXT NOT NULL,
  tree TEXT NOT NULL DEFAULT '',
  base TEXT NOT NULL DEFAULT '',
  batch_size INTEGER NOT NULL CHECK(batch_size >= 0),
  bisect_depth INTEGER NOT NULL CHECK(bisect_depth >= 0),
  started_at TEXT NOT NULL,
  finished_at TEXT NOT NULL,
  outcome TEXT NOT NULL CHECK(outcome IN ('pass','fail','moved')),
  UNIQUE(repo, candidate, started_at)
) STRICT;
CREATE INDEX IF NOT EXISTS land_checks_repo_started ON land_checks(repo, started_at);
CREATE TABLE IF NOT EXISTS main_red(
  repo TEXT NOT NULL,
  sha TEXT NOT NULL,
  red_at TEXT NOT NULL,
  green_at TEXT,
  cause TEXT NOT NULL DEFAULT '',
  cases TEXT NOT NULL DEFAULT '',
  revert_sha TEXT NOT NULL DEFAULT '',
  PRIMARY KEY(repo, sha)
) STRICT;";

const ADDED_TO_LANDINGS: [&str; 3] = ["first_commit_at", "check_started_at", "check_finished_at"];

const USAGE: &str = "usage: threads land-train record|check|red ...";

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, _: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv.get(1).map(String::as_str) {
        Some("record") => record(&argv[2..], context),
        Some("check") => check(&argv[2..], context),
        Some("red") => red(&argv[2..], context),
        _ => Err(Failure::Usage(USAGE.into())),
    };
    report(&argv[0], result, out, err)
}

fn time(args: &Args, flag: &str) -> Result<Option<String>, Failure> {
    let Some(text) = args.get(flag) else { return Ok(None) };
    let seconds: f64 = text.parse().map_err(|_| Failure::Usage(format!("{flag} {text} is not epoch seconds")))?;
    Ok(Some(Timestamp::from_unix_seconds_f64(seconds).iso()))
}

fn required(args: &Args, flag: &str) -> Result<String, Failure> {
    args.get(flag).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| Failure::Usage(format!("{flag} is required")))
}

fn repo_name(args: &Args) -> Result<String, Failure> {
    Ok(CATALOG.canonical(&required(args, "--repo")?).to_owned())
}

fn count(args: &Args, flag: &str) -> Result<i64, Failure> {
    let Some(text) = args.get(flag) else { return Ok(0) };
    text.parse::<i64>().ok().filter(|value| *value >= 0).ok_or_else(|| Failure::Usage(format!("{flag} {text} is not a count")))
}

fn add_missing_columns(db: &Connection) -> rusqlite::Result<()> {
    let present: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('landings')")?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
    for column in ADDED_TO_LANDINGS.iter().filter(|column| !present.iter().any(|name| name == *column)) {
        db.execute_batch(&format!("ALTER TABLE landings ADD COLUMN {column} TEXT"))?;
    }
    Ok(())
}

fn write(context: &Context, statement: impl FnOnce(&Connection) -> rusqlite::Result<usize>) -> Outcome {
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    add_missing_columns(&tx)?;
    statement(&tx)?;
    tx.commit()?;
    Ok(String::new())
}

fn record(argv: &[String], context: &Context) -> Outcome {
    let valued = ["--repo", "--lane", "--branch", "--tip", "--queued-at", "--candidate", "--tree", "--batch-size", "--bisect-depth", "--attempts", "--landed-at", "--landed-sha", "--verdict", "--detail", "--first-commit-at", "--check-started-at", "--check-finished-at"];
    let args = Args::parse(argv, &valued, &[])?;
    let (repo, lane, tip) = (repo_name(&args)?, required(&args, "--lane")?, required(&args, "--tip")?);
    let queued = time(&args, "--queued-at")?.ok_or_else(|| Failure::Usage("--queued-at is required".into()))?;
    let verdict = required(&args, "--verdict")?;
    if !["landed", "conflict", "failed"].contains(&verdict.as_str()) {
        return Err(Failure::Usage(format!("--verdict {verdict} is not landed, conflict or failed")));
    }
    let landed = time(&args, "--landed-at")?;
    let (first_commit, check_started, check_finished) = (time(&args, "--first-commit-at")?, time(&args, "--check-started-at")?, time(&args, "--check-finished-at")?);
    let counts = (count(&args, "--batch-size")?, count(&args, "--bisect-depth")?, count(&args, "--attempts")?);
    let text = |flag| args.get(flag).unwrap_or_default().to_owned();
    write(context, |db| {
        db.execute(
            "INSERT INTO landings(repo, lane, branch, tip, queued_at, candidate, tree, batch_size, bisect_depth, attempts, actor, landed_at, landed_sha, verdict, detail, first_commit_at, check_started_at, check_finished_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18) \
             ON CONFLICT(repo, lane, tip, queued_at) DO UPDATE SET branch = excluded.branch, candidate = excluded.candidate, tree = excluded.tree, \
             batch_size = excluded.batch_size, bisect_depth = excluded.bisect_depth, attempts = excluded.attempts, actor = excluded.actor, \
             landed_at = excluded.landed_at, landed_sha = excluded.landed_sha, verdict = excluded.verdict, detail = excluded.detail, \
             first_commit_at = coalesce(excluded.first_commit_at, first_commit_at), check_started_at = coalesce(excluded.check_started_at, check_started_at), \
             check_finished_at = coalesce(excluded.check_finished_at, check_finished_at)",
            params![repo, lane, text("--branch"), tip, queued, text("--candidate"), text("--tree"), counts.0, counts.1, counts.2, context.by(&args), landed, args.get("--landed-sha"), verdict, text("--detail"), first_commit, check_started, check_finished],
        )
    })
}

fn check(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--repo", "--candidate", "--tree", "--base", "--batch-size", "--bisect-depth", "--started-at", "--finished-at", "--outcome"], &[])?;
    let (repo, candidate, outcome) = (repo_name(&args)?, required(&args, "--candidate")?, required(&args, "--outcome")?);
    if !["pass", "fail", "moved"].contains(&outcome.as_str()) {
        return Err(Failure::Usage(format!("--outcome {outcome} is not pass, fail or moved")));
    }
    let started = time(&args, "--started-at")?.ok_or_else(|| Failure::Usage("--started-at is required".into()))?;
    let finished = time(&args, "--finished-at")?.ok_or_else(|| Failure::Usage("--finished-at is required".into()))?;
    let (batch, depth) = (count(&args, "--batch-size")?, count(&args, "--bisect-depth")?);
    let text = |flag| args.get(flag).unwrap_or_default().to_owned();
    write(context, |db| {
        db.execute(
            "INSERT INTO land_checks(repo, candidate, tree, base, batch_size, bisect_depth, started_at, finished_at, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(repo, candidate, started_at) DO UPDATE SET finished_at = excluded.finished_at, outcome = excluded.outcome",
            params![repo, candidate, text("--tree"), text("--base"), batch, depth, started, finished, outcome],
        )
    })
}

fn red(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--repo", "--sha", "--red-at", "--green-at", "--cause", "--cases", "--revert-sha"], &[])?;
    let (repo, sha) = (repo_name(&args)?, required(&args, "--sha")?);
    let red_at = time(&args, "--red-at")?.ok_or_else(|| Failure::Usage("--red-at is required".into()))?;
    let green_at = time(&args, "--green-at")?;
    let text = |flag| args.get(flag).unwrap_or_default().to_owned();
    write(context, |db| {
        db.execute(
            "INSERT INTO main_red(repo, sha, red_at, green_at, cause, cases, revert_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(repo, sha) DO UPDATE SET green_at = coalesce(excluded.green_at, green_at), cause = excluded.cause, cases = excluded.cases, revert_sha = excluded.revert_sha",
            params![repo, sha, red_at, green_at, text("--cause"), text("--cases"), text("--revert-sha")],
        )
    })
}
