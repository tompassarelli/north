//! `north threads claim` and `north threads list` against the ledger on this host.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use serde_json::json;

use super::issues::Issues;
use super::ledger::{self, Taken};
use super::{Claim, Issue, OVERDUE_RATIO, Standing};
use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;

pub(crate) const VERBS: [&str; 2] = ["claim", "list"];

pub(crate) struct Context<'a> {
    pub database: &'a Path,
    pub actor: String,
    /// Why native items are hidden from this caller, when they are.
    pub no_native: Option<&'static str>,
    pub issues: &'a dyn Issues,
    /// The owners of the sessions alive now, or `None` when no fleet could be read.
    pub alive: Option<BTreeSet<String>>,
    pub now: i64,
}

struct Args {
    positional: Vec<String>,
    by: Option<String>,
    eta: Option<String>,
    json: bool,
}

fn args(argv: &[String]) -> Result<Args, String> {
    let mut parsed = Args { positional: Vec::new(), by: None, eta: None, json: false };
    let mut rest = argv.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = arg.split_once('=').map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value.to_owned())));
        let mut value = || inline.clone().or_else(|| rest.next().cloned()).ok_or_else(|| format!("{flag} needs a value"));
        match flag {
            "--by" => parsed.by = Some(value()?),
            "--eta" => parsed.eta = Some(value()?),
            "--json" if inline.is_none() => parsed.json = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ => parsed.positional.push(arg.clone()),
        }
    }
    Ok(parsed)
}

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub(crate) fn run(argv: &[String], context: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = args(&argv[1..]).and_then(|parsed| match argv[0].as_str() {
        "claim" => claim(&parsed, context),
        _ => list(&parsed, context),
    });
    match result {
        Ok(text) => i32::from(write!(out, "{text}").is_err()),
        Err(why) => {
            let _ = writeln!(err, "NorthThreads: {why}");
            1
        }
    }
}

fn issue_of(db: &rusqlite::Connection, item: &str, github: &BTreeMap<(String, u64), Issue>) -> Issue {
    match CATALOG.parse_ref(item) {
        Ok((ns, number)) if CATALOG.is_native(&ns) => ledger::native(db, &ns, number),
        Ok(key) => github.get(&key).copied().unwrap_or(Issue::Unread),
        Err(_) => Issue::Unread,
    }
}

fn github_items<'a>(items: impl Iterator<Item = &'a str>) -> BTreeSet<(String, u64)> {
    items.filter_map(|item| CATALOG.parse_ref(item).ok()).filter(|(repo, _)| !CATALOG.is_native(repo)).collect()
}

fn claim(parsed: &Args, context: &Context) -> Result<String, String> {
    let [id] = parsed.positional.as_slice() else { return Err("usage: threads claim REPO#N [--by NAME] [--eta MINUTES]".into()) };
    let (repo, number) = CATALOG.parse_ref(id)?;
    if let Some(reason) = context.no_native.filter(|_| CATALOG.is_native(&repo)) {
        return Err(format!("native items are not readable here: {reason}"));
    }
    let eta = parsed.eta.as_deref().map(|eta| eta.parse::<i64>().ok().filter(|eta| *eta > 0).ok_or_else(|| format!("--eta {eta} is not a whole number of minutes"))).transpose()?;
    let by = parsed.by.clone().unwrap_or_else(|| context.actor.clone());
    let item = format!("{repo}#{number}");
    let mut db = ledger::open(context.database, true)?;
    let github = context.issues.read(&github_items(std::iter::once(item.as_str())));
    let issue = issue_of(&db, &item, &github);
    Ok(match ledger::take(&mut db, &item, &by, eta, issue, context.alive.as_ref(), context.now)? {
        Taken::Fresh | Taken::Renewed => format!("{by} owns {item}\n"),
        Taken::Voided { previous, why } => format!("{by} owns {item}; the claim by {} was void because {why}\n", previous.owner),
        Taken::Takeover { previous, idle_since } => {
            format!("{by} owns {item}, taken over from {}: stale, no owner event since {}\n", previous.owner, Timestamp::from_unix_seconds(idle_since).iso())
        }
        Taken::Refused(holder) => return Err(format!("{item} is already owned by {}", holder.owner)),
    })
}

/// Every claim that still stands, with its standing; a void claim holds nothing, so none shows.
pub(crate) fn standing_claims(context: &Context) -> Result<Vec<(Claim, Standing)>, String> {
    let db = ledger::open(context.database, false)?;
    let claims: Vec<Claim> = ledger::claims(&db)?
        .into_iter()
        .filter(|claim| context.no_native.is_none() || CATALOG.parse_ref(&claim.item).is_ok_and(|(repo, _)| !CATALOG.is_native(&repo)))
        .collect();
    let github = context.issues.read(&github_items(claims.iter().map(|claim| claim.item.as_str())));
    Ok(claims
        .into_iter()
        .map(|claim| {
            let standing = claim.standing(issue_of(&db, &claim.item, &github), context.alive.as_ref(), context.now);
            (claim, standing)
        })
        .filter(|(_, standing)| !matches!(standing, Standing::Void(_)))
        .collect())
}

fn list(parsed: &Args, context: &Context) -> Result<String, String> {
    if !parsed.positional.is_empty() || parsed.by.is_some() || parsed.eta.is_some() {
        return Err("usage: threads list [--json]".into());
    }
    let claims = standing_claims(context)?;
    let clock = |claim: &Claim| (context.now - claim.since).max(0) / 60;
    if parsed.json {
        let rows: Vec<_> = claims
            .iter()
            .map(|(claim, standing)| {
                let standing = if matches!(standing, Standing::Live) { "live" } else { "stale" };
                json!({"item": claim.item, "owner": claim.owner, "since": Timestamp::from_unix_seconds(claim.since).iso(), "clock_min": clock(claim), "eta_min": claim.eta_min, "standing": standing})
            })
            .collect();
        return Ok(format!("{}\n", serde_json::Value::Array(rows)));
    }
    let mut rows = vec![["item", "owner", "clock", "eta", "flag"].map(str::to_owned)];
    for (claim, standing) in &claims {
        let flag = match (standing, claim.eta_min) {
            (Standing::Stale { .. }, _) => "STALE",
            (_, Some(eta)) if clock(claim) as f64 > *OVERDUE_RATIO * eta as f64 => "OVERDUE",
            _ => "-",
        };
        let eta = claim.eta_min.map_or_else(|| "-".to_owned(), |eta| format!("{eta}m"));
        rows.push([claim.item.clone(), claim.owner.clone(), format!("{}m", clock(claim)), eta, flag.to_owned()]);
    }
    let widths: Vec<usize> = (0..5).map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0)).collect();
    Ok(rows.iter().map(|row| format!("{}\n", row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end())).collect())
}
