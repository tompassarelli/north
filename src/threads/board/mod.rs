//! `north threads show` and `north threads ready`: one issue with its blockers, owners, handoffs
//! and runs, and the open issues nothing blocks and nobody holds.
pub(crate) mod issues;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io::{Read, Write};

use rusqlite::{Connection, OptionalExtension, params};

use self::issues::Board;
use super::claims::{self, Claim, Standing};
use super::verb::{Args, Context, Failure, Outcome, item_ref, report};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson;
use crate::threads::catalog::CATALOG;

pub(crate) const VERBS: [&str; 2] = ["show", "ready"];

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub(crate) fn run(argv: &[String], context: &Context, _stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv[0].as_str() {
        "show" => show(&argv[1..], context),
        _ => ready(&argv[1..], context, err),
    };
    report(&argv[0], result, out, err)
}

fn show(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    let [id] = args.positional.as_slice() else { return Err(Failure::Usage("usage: threads show REPO#N".into())) };
    let (repo, number) = item_ref(id, context)?;
    let item = format!("{repo}#{number}");
    let board = Board::open(context)?;
    let issue = board.one_issue(&repo, number)?;
    let mut shown = format!("{item}  {}\n  state: {}\n", issue.title, issue.state.to_lowercase());
    if !issue.blockers.is_empty() {
        let blockers: Vec<String> = issue.blockers.iter().map(|blocker| format!("{} ({})", blocker.reference, blocker.state.to_lowercase())).collect();
        shown += &format!("  blocked by: {}\n", blockers.join(", "));
    }
    let db = board.db();
    if CATALOG.is_native(&repo) {
        shown += &native_detail(&db, &repo, number, &item)?;
    }
    shown += &owners(&db, &item, context)?;
    shown += &runs(&db, &item)?;
    if !CATALOG.is_native(&repo) {
        shown += &context.outside.archives(&repo, number);
    }
    Ok(shown)
}

/// A native item's due date, body and notes.
fn native_detail(db: &Connection, repo: &str, number: u64, item: &str) -> Outcome {
    let mut shown = String::new();
    let (due, body): (Option<String>, String) = db.query_row("SELECT due, body FROM items WHERE ns = ?1 AND number = ?2", params![repo, number as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if let Some(due) = due.filter(|due| !due.is_empty()) {
        shown += &format!("  due: {due}\n");
    }
    if !body.is_empty() {
        shown += &format!("  {}\n", body.replace('\n', "\n  "));
    }
    let mut notes = db.prepare("SELECT ts, data FROM events WHERE item = ?1 AND kind = 'note' AND json_extract(data, '$.text') IS NOT NULL ORDER BY seq")?;
    for note in notes.query_map([item], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
        let (ts, data) = note?;
        shown += &format!("  note {ts}  {}\n", printed(&data, "text"));
    }
    Ok(shown)
}

/// Who holds the item now and every claim, release and handoff of it.
fn owners(db: &Connection, item: &str, context: &Context) -> Outcome {
    let mut shown = String::new();
    let claim: Option<(String, String, Option<i64>)> = db.query_row("SELECT owner, since, eta_min FROM claims WHERE item = ?1", [item], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
    if let Some((owner, since, eta)) = claim {
        let since = Timestamp::parse(&since).map_or(context.now.unix_seconds(), Timestamp::unix_seconds);
        let eta = eta.filter(|eta| *eta != 0).map_or_else(String::new, |eta| format!(" of {eta}m ETA"));
        shown += &format!("  owner: {owner} for {}m{eta}\n", (context.now.unix_seconds() - since).div_euclid(60));
    }
    let mut hands = db.prepare("SELECT ts, kind, actor, data FROM events WHERE item = ?1 AND kind IN ('claim','release','handoff') ORDER BY seq")?;
    let hands: Vec<(String, String, String, String)> = hands.query_map([item], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?.collect::<Result<_, _>>()?;
    if !hands.is_empty() {
        shown += "  owners:\n";
        for (ts, kind, actor, data) in hands {
            let what = if kind == "handoff" { format!("{} -> {}", printed(&data, "from"), printed(&data, "to")) } else { actor };
            shown += &format!("    {ts}  {kind:<8} {what}\n");
        }
    }
    Ok(shown)
}

/// The worker runs spent on the item, oldest first.
fn runs(db: &Connection, item: &str) -> Outcome {
    let mut select = db.prepare("SELECT ended, agent, tier, follows, minutes, eta_min, outcome, report, note FROM runs WHERE item = ?1 ORDER BY ended")?;
    let runs: Vec<Run> = select
        .query_map([item], |row| Ok(Run { ended: row.get(0)?, agent: row.get(1)?, tier: row.get(2)?, follows: row.get(3)?, minutes: row.get(4)?, eta: row.get(5)?, outcome: row.get(6)?, report: row.get(7)?, note: row.get(8)? }))?
        .collect::<Result<_, _>>()?;
    if runs.is_empty() {
        return Ok(String::new());
    }
    let mut shown = format!("  runs: {}, {}m in all\n", runs.len(), runs.iter().map(|run| run.minutes).sum::<i64>());
    for run in &runs {
        let eta = run.eta.filter(|eta| *eta != 0).map_or_else(String::new, |eta| format!("/{eta}m ETA"));
        let follows = run.follows.as_deref().filter(|follows| !follows.is_empty()).map_or_else(String::new, |follows| format!(" follows {follows}"));
        let note = run.note.as_deref().filter(|note| !note.is_empty()).map_or_else(String::new, |note| format!("  note {note}"));
        let ended: String = run.ended.chars().take(16).collect();
        let summary: String = run.report.chars().take(80).collect();
        shown += &format!("    {ended}  {} {}{follows}  {}m{eta}  {}: {summary}{note}\n", run.agent, run.tier, run.minutes, run.outcome);
    }
    Ok(shown)
}

struct Run {
    ended: String,
    agent: String,
    tier: String,
    follows: Option<String>,
    minutes: i64,
    eta: Option<i64>,
    outcome: String,
    report: String,
    note: Option<String>,
}

/// One field of an event's JSON data, printed the way Python's `print` shows it.
fn printed(data: &str, key: &str) -> String {
    let Ok(value) = pyjson::loads(data.as_bytes()) else { return String::new() };
    match value.get(key) {
        Some(field) => field.as_string().map_or_else(|| pyjson::dumps(field), |text| text.lossy().into_owned()),
        None => String::new(),
    }
}

/// Open issues with no open blocker whose claim is not live and whose owner is not alive now; a
/// repository that cannot be read is skipped with a line on `err`.
fn ready(argv: &[String], context: &Context, err: &mut dyn Write) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads ready".into()));
    }
    let held = held(context)?;
    let board = Board::open(context)?;
    let visible = if context.no_native.is_some() { &[][..] } else { &CATALOG.native[..] };
    let mut listed = String::new();
    for repo in CATALOG.repos.iter().chain(visible) {
        let mut issues = match board.open_issues(repo) {
            Ok(issues) => issues,
            Err(Failure::Refused(why) | Failure::Usage(why)) => {
                let _ = writeln!(err, "NorthThreads: skipped {repo}: {why}");
                continue;
            }
        };
        issues.sort_by_key(|issue| issue.number);
        for issue in issues {
            let item = format!("{repo}#{}", issue.number);
            let local = if CATALOG.is_native(repo) { Vec::new() } else { board.local_blockers(&item)? };
            if held.contains_key(&item) || issue.blockers.iter().chain(&local).any(|blocker| blocker.state == "OPEN") {
                continue;
            }
            listed += &format!("{item}\t{}\n", issue.title);
        }
    }
    Ok(listed)
}

/// The claims that hold their issue: live ones, and stale ones whose owner is alive now.
pub(crate) fn held(context: &Context) -> Result<BTreeMap<String, Claim>, Failure> {
    let alive = context.outside.alive_owners();
    let claims_context = claims::verbs::Context { database: context.database, actor: context.actor.clone(), no_native: context.no_native, issues: context.outside, alive: alive.clone(), now: context.now.unix_seconds() };
    let alive = alive.unwrap_or_default();
    Ok(claims::verbs::standing_claims(&claims_context)?
        .into_iter()
        .filter(|(claim, standing)| matches!(standing, Standing::Live) || alive.contains(&claim.owner))
        .map(|(claim, _)| (claim.item.clone(), claim))
        .collect())
}
