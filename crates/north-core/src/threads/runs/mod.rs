//! `north threads ingest` and `north threads run-ids`: north worker-ledger's JSON rows into the runs,
//! claims and usage tables, and the run ids already recorded.
pub mod brief;
mod costs;
mod record;
mod row;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};

use rusqlite::types::Value as Sql;
use rusqlite::{OptionalExtension, params, params_from_iter};

use self::record::{Ingest, Tally};
use self::row::{Row, bind, value};
use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson::{self, Value};
use crate::threads::catalog::CATALOG;

pub const VERBS: [&str; 2] = ["ingest", "run-ids"];
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
        _ => run_ids(&argv[1..], context),
    };
    report(&argv[0], result, out, err)
}

/// Every recorded run's agent id; with --settled, only finished worker runs not still waiting to land, and with --ended each id is followed by a tab and the time its run was recorded as ending.
fn run_ids(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &["--settled", "--ended"])?;
    if !args.positional.is_empty() || (args.has("--ended") && !args.has("--settled")) {
        return Err(Failure::Usage("usage: threads run-ids [--settled [--ended]]".into()));
    }
    let db = open(context.database, false)?;
    let since = Timestamp::from_unix_seconds(context.now.unix_seconds() - LANDING_DAYS * 86400).iso();
    let settled = args.has("--settled");
    let filter = if settled { " WHERE role = \"worker\" AND outcome != \"running\" AND NOT (expects_landing = 1 AND landed_at IS NULL AND ended >= ?)" } else { "" };
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

/// Records every row in one transaction, then the usage readouts, the run costs they price and the
/// categories issue labels supply; prints what it recorded to `err`.
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
    if !readouts.is_empty() {
        costs::record(&mut db, &readouts)?;
    }
    if db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'usage'", [], |_| Ok(())).optional()?.is_some() {
        costs::allocate(&mut db, &context.outside.prices()?)?;
    }
    let mut labels = BTreeMap::new();
    let labeled: Vec<(&str, &Value)> = tally.unlabeled.iter().filter_map(|(agent, item)| Some((label_category(context, item, &mut labels)?, agent))).collect();
    let tx = immediate(&mut db)?;
    for (category, agent) in labeled {
        tx.execute("UPDATE runs SET category = ? WHERE agent = ?", params![category, bind(Some(agent))?])?;
    }
    tx.commit()?;
    let backfilled = if tally.backfilled > 0 { format!(", {} backfilled", tally.backfilled) } else { String::new() };
    let unpriced = tally.unpriced.into_iter().collect::<Vec<_>>().join(", ");
    let unpriced = if unpriced.is_empty() { unpriced } else { format!("; token weight for models without a list price: {unpriced}") };
    writeln!(err, "NorthThreads: {} runs recorded{backfilled} in {}{unpriced}", tally.added, context.database.display()).map_err(|error| Failure::Refused(error.to_string()))?;
    Ok(String::new())
}

/// Category from the item's GitHub labels by LABEL_CATEGORY, one label listing per repo per ingest.
fn label_category(context: &Context, item: &str, cache: &mut BTreeMap<String, BTreeMap<u64, BTreeSet<String>>>) -> Option<&'static str> {
    let (repo, number) = CATALOG.parse_ref(item).ok()?;
    if CATALOG.is_native(&repo) {
        return None;
    }
    let labels = cache.entry(repo.clone()).or_insert_with(|| {
        let listed = context.gh(&["issue", "list", "-R", &format!("{}/{repo}", CATALOG.owner), "--state", "all", "--limit", "1000", "--json", "number,labels"]);
        listed.ok().and_then(|text| issue_labels(&text)).unwrap_or_default()
    });
    let labels = labels.get(&number)?;
    LABEL_CATEGORY.iter().find(|(label, _)| labels.contains(*label)).map(|(_, category)| *category)
}

fn issue_labels(listed: &str) -> Option<BTreeMap<u64, BTreeSet<String>>> {
    let issues: serde_json::Value = serde_json::from_str(listed).ok()?;
    let labels = |issue: &serde_json::Value| issue["labels"].as_array()?.iter().map(|label| label["name"].as_str().map(str::to_owned)).collect::<Option<BTreeSet<_>>>();
    issues.as_array()?.iter().map(|issue| Some((issue["number"].as_u64()?, labels(issue)?))).collect()
}
