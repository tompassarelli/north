//! `north threads unowned`: the open smashcraft and wisp issues with an unchecked box, each with
//! its owner, its block or UNOWNED, priority:now first.
pub mod branches;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::params;
use serde_json::Value as Json;

use self::branches::{Failed, names_issue};
use super::board::held;
use super::verb::{Args, Context, Failure, Outcome, event, immediate, open, report};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson::Value;
use crate::threads::catalog::CATALOG;
use crate::threads::withheld::withheld;

pub const VERBS: [&str; 1] = ["unowned"];
const STAFFED: [&str; 2] = ["smashcraft", "wisp"];
const PRIORITIES: [&str; 4] = ["priority:now", "-", "priority:next", "priority:later"];

static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));

/// Runs the verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, _stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = unowned(&argv[1..], context, err);
    report(&argv[0], result, out, err)
}

struct Row {
    rank: usize,
    repo: usize,
    number: u64,
    priority: &'static str,
    state: String,
    boxes: usize,
    title: String,
}

/// A claim made before its issue's lane failed Autoland, released by the failure.
struct Stale<'a> {
    item: String,
    owner: String,
    branch: &'a str,
    failed: &'a Failed,
}

fn unowned(argv: &[String], context: &Context, err: &mut dyn Write) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads unowned".into()));
    }
    let claims = held(context)?;
    let mut db = open(context.database, true)?;
    let blocks: BTreeMap<String, String> = db.prepare("SELECT item, reason FROM blocks")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut scanned = Vec::new();
    for repo in STAFFED {
        let listed = context.gh(&["issue", "list", "-R", &format!("{}/{repo}", CATALOG.owner), "--state", "open", "--limit", "500", "--json", "number,title,labels,body,comments"])?;
        let issues: Vec<Json> = serde_json::from_str(&listed).map_err(|_| Failure::Refused("gh issue list: unreadable reply".into()))?;
        scanned.push((repo, issues, branches::lanes(context, repo, err)?));
    }
    let (mut rows, mut stale) = (Vec::new(), Vec::new());
    for (index, (repo, issues, lanes)) in scanned.iter().enumerate() {
        for issue in issues {
            let boxes = BOX.find_iter(issue["body"].as_str().unwrap_or_default()).count();
            let Some(number) = issue["number"].as_u64().filter(|_| boxes > 0) else { continue };
            let item = format!("{repo}#{number}");
            let labels: Vec<&str> = issue["labels"].as_array().map(|labels| labels.iter().filter_map(|label| label["name"].as_str()).collect()).unwrap_or_default();
            let priority = PRIORITIES.into_iter().find(|priority| labels.contains(priority)).unwrap_or("-");
            let lane = lanes.failed.iter().find(|(branch, failed)| names_issue(branch, &failed.messages, repo, number));
            let mut claim = claims.get(&item);
            if let (Some(held), Some((branch, failed))) = (claim, lane)
                && Timestamp::from_unix_seconds(held.since).iso() < failed.at
            {
                stale.push(Stale { item: item.clone(), owner: held.owner.clone(), branch, failed });
                claim = None;
            }
            let owner = claim.map(|claim| claim.owner.clone()).or_else(|| lanes.open.iter().find(|(branch, messages)| names_issue(branch, messages, repo, number)).map(|(branch, _)| format!("branch:{branch}")));
            let block = blocks.get(&item).filter(|reason| !reason.is_empty()).cloned().or_else(|| withheld(&labels, issue["comments"].as_array().map_or(&[][..], Vec::as_slice), context.now.unix_seconds_f64()));
            let state = owner.unwrap_or_else(|| block.map_or_else(|| "UNOWNED".to_owned(), |block| format!("blocked:{block}")));
            let note = lane.filter(|_| state == "UNOWNED").map_or_else(String::new, |(branch, failed)| format!("\tlane failed: {} ({branch})", failed.reason));
            let title = format!("{}{note}", issue["title"].as_str().unwrap_or_default());
            rows.push(Row { rank: PRIORITIES.iter().position(|known| *known == priority).unwrap_or(1), repo: index, number, priority, state, boxes, title });
        }
    }
    let tx = immediate(&mut db)?;
    for released in &stale {
        tx.execute("DELETE FROM claims WHERE item = ?1 AND owner = ?2", params![released.item, released.owner])?;
        let data = Value::object([
            ("owner", released.owner.as_str().into()),
            ("lane_failed", released.branch.into()),
            ("reason", released.failed.reason.as_str().into()),
            ("failed_at", released.failed.at.as_str().into()),
        ]);
        event(&tx, Some(&released.item), "release", "threads", data, context.now)?;
    }
    tx.commit()?;
    rows.sort_by_key(|row| (row.rank, row.repo, row.number));
    Ok(listing(&rows))
}

fn listing(rows: &[Row]) -> String {
    let mut listed: String = rows.iter().map(|row| format!("{}#{}\t{}\t{}\t{}\t{}\n", STAFFED[row.repo], row.number, row.priority, row.state, row.boxes, row.title)).collect();
    let now: Vec<&Row> = rows.iter().filter(|row| row.priority == "priority:now").collect();
    let unowned = now.iter().filter(|row| row.state == "UNOWNED").count();
    let blocked = now.iter().filter(|row| row.state.starts_with("blocked:")).count();
    listed += &format!("priority:now unowned={unowned} blocked={blocked} owned={}\n", now.len() - unowned - blocked);
    listed
}
