//! `north threads job-run`, `job-runs` and `queue`: scheduled job runs, and the queue of jobs for
//! machines that may be asleep.
mod queue;
#[cfg(test)]
mod tests;

use std::io::{Read, Write};

use rusqlite::params;

use super::verb::{Args, Context, Failure, Outcome, immediate, integer, open, report, table, text_or_null};
use crate::platform::pyjson::Value;

pub const VERBS: [&str; 3] = ["job-run", "job-runs", "queue"];

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv[0].as_str() {
        "job-run" => job_run(&argv[1..], context),
        "job-runs" => job_runs(&argv[1..], context),
        _ => queue::run(&argv[1..], context, stdin),
    };
    report(&argv[0], result, out, err)
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
        return Ok(format!("{}\n", crate::platform::pyjson::dumps(&Value::Array(rows))));
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
