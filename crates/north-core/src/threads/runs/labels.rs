//! `north threads label-runs`: the category of recent worker runs whose brief named none, from their issue's labels.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use rusqlite::params;
use rusqlite::types::Value as Sql;

use super::{LABEL_CATEGORY, LANDING_DAYS};
use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;
use crate::threads::verb::{Args, Context, Failure, Outcome, immediate, open};

/// Sets the category of each worker run that ended in the last LANDING_DAYS days still `unknown`,
/// listing each repo's issues once with no write transaction open, because one listing takes
/// seconds and `ingest` holds the ledger's exclusive lock.
pub(super) fn label_runs(argv: &[String], context: &Context, err: &mut dyn Write) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads label-runs".into()));
    }
    let mut db = open(context.database, true)?;
    let since = Timestamp::from_unix_seconds(context.now.unix_seconds() - LANDING_DAYS * 86400).iso();
    let pending: Vec<(Sql, String)> = db
        .prepare("SELECT agent, item FROM runs WHERE role = 'worker' AND category = 'unknown' AND item IS NOT NULL AND ended >= ?")?
        .query_map([since], |row| Ok((row.get::<_, Sql>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut listings = BTreeMap::new();
    let labeled: Vec<(&str, &Sql)> = pending.iter().filter_map(|(agent, item)| Some((label_category(context, item, &mut listings)?, agent))).collect();
    let tx = immediate(&mut db)?;
    for (category, agent) in &labeled {
        tx.execute("UPDATE runs SET category = ? WHERE agent = ? AND category = 'unknown'", params![category, agent])?;
    }
    tx.commit()?;
    writeln!(err, "NorthThreads: {} of {} unknown runs labeled in {}", labeled.len(), pending.len(), context.database.display()).map_err(|error| Failure::Refused(error.to_string()))?;
    Ok(String::new())
}

/// Category from the item's GitHub labels by LABEL_CATEGORY, one label listing per repo.
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
