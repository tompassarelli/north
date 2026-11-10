//! `north threads claim` and `north threads list` against the ledger on this host.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use serde_json::json;

use super::issues::Issues;
use super::ledger::{self, Taken};
use super::lifecycle::{self, Moved};
use super::{Claim, Issue, OVERDUE_RATIO, Standing, box_slug, names_box, split_key};
use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;

pub const VERBS: [&str; 3] = ["claim", "list", HOLDER];

/// The read that answers who holds the items or owners it names from the claims table alone, never reading GitHub or a fleet.
pub const HOLDER: &str = "holder";

/// What a refusal by a live holder says, which `north fleet launch` tells from a ledger it could not read.
pub const OWNED: &str = "is already owned by";

pub struct Context<'a> {
    pub database: &'a Path,
    pub actor: String,
    /// Why native items are hidden from this caller, when they are.
    pub no_native: Option<&'static str>,
    pub issues: &'a dyn Issues,
    /// The owners of the sessions alive now, or `None` when no fleet could be read.
    pub alive: Option<BTreeSet<String>>,
    pub now: i64,
    /// Whether `database` is this host's read-only copy of the ledger, which records no phase.
    pub replica: bool,
}

struct Args {
    positional: Vec<String>,
    by: Option<String>,
    eta: Option<String>,
    json: bool,
    all: bool,
    owners: Vec<String>,
    box_text: Option<String>,
    parents: Vec<String>,
}

fn args(argv: &[String]) -> Result<Args, String> {
    let mut parsed = Args { positional: Vec::new(), by: None, eta: None, json: false, all: false, owners: Vec::new(), box_text: None, parents: Vec::new() };
    let mut rest = argv.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = arg.split_once('=').map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value.to_owned())));
        let mut value = || inline.clone().or_else(|| rest.next().cloned()).ok_or_else(|| format!("{flag} needs a value"));
        match flag {
            "--by" => parsed.by = Some(value()?),
            "--eta" => parsed.eta = Some(value()?),
            "--owner" => parsed.owners.push(value()?),
            "--parent" => parsed.parents.push(value()?),
            "--box" => parsed.box_text = Some(value()?),
            "--json" if inline.is_none() => parsed.json = true,
            "--all" if inline.is_none() => parsed.all = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ => parsed.positional.push(arg.clone()),
        }
    }
    Ok(parsed)
}

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = args(&argv[1..]).and_then(|parsed| match argv[0].as_str() {
        "claim" => claim(&parsed, context),
        HOLDER => holder(&parsed, context),
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

/// Refuses a box slug the issue's body lacks, ticked or not: a claim on a box its issue lacks holds every box of it in dispatch. A native item or an unreadable body refuses nothing.
fn box_of_issue(issues: &dyn Issues, (repo, number): (&str, u64), slug: &str) -> Result<(), String> {
    let body = (!CATALOG.is_native(repo)).then(|| issues.body(repo, number)).flatten();
    match body {
        Some(body) if !names_box(&body, slug) => Err(format!("--box {slug} names no box of {repo}#{number}; a box another issue holds is claimed on that issue, since a claim on a box its issue lacks holds every box of it")),
        _ => Ok(()),
    }
}

fn claim(parsed: &Args, context: &Context) -> Result<String, String> {
    let [id] = parsed.positional.as_slice() else { return Err("usage: threads claim REPO#N [--box TEXT] [--by NAME] [--parent NAME ...] [--eta MINUTES]".into()) };
    let (repo, number) = CATALOG.parse_ref(id)?;
    if let Some(reason) = context.no_native.filter(|_| CATALOG.is_native(&repo)) {
        return Err(format!("native items are not readable here: {reason}"));
    }
    let eta = parsed.eta.as_deref().map(|eta| eta.parse::<i64>().ok().filter(|eta| *eta > 0).ok_or_else(|| format!("--eta {eta} is not a whole number of minutes"))).transpose()?;
    let by = parsed.by.clone().unwrap_or_else(|| context.actor.clone());
    let item = format!("{repo}#{number}");
    let slug = parsed.box_text.as_deref().map(|text| box_slug(text).ok_or_else(|| format!("--box {text} names no word of a box"))).transpose()?;
    if let Some(slug) = &slug {
        box_of_issue(context.issues, (&repo, number), slug)?;
    }
    let mut db = ledger::open(context.database, true)?;
    let github = context.issues.read(&github_items(std::iter::once(item.as_str())));
    let issue = issue_of(&db, &item, &github);
    let ancestors: BTreeSet<String> = parsed.parents.iter().cloned().collect();
    let claimant = ledger::Claimant { name: &by, ancestors: &ancestors };
    let (taken, held) = ledger::take(&mut db, (&item, slug.as_deref()), claimant, eta, issue, context.alive.as_ref(), context.now)?;
    drop(db);
    let (_, claimed) = split_key(&held);
    let shown = claimed.map_or_else(|| item.clone(), |slug| format!("{item} box {slug}"));
    let (printed, cause) = match taken {
        Taken::Fresh => (format!("{by} owns {shown}\n"), format!("claimed by {by}")),
        Taken::Renewed => (format!("{by} owns {shown}\n"), format!("renewed by {by}")),
        Taken::Voided { previous, why } => (
            format!("{by} owns {shown}; the claim by {} was void because {why}\n", previous.owner),
            format!("taken by {by}: the claim by {} was void because {why}", previous.owner),
        ),
        Taken::Takeover { previous, idle_since } => {
            let idle = Timestamp::from_unix_seconds(idle_since).iso();
            (format!("{by} owns {shown}, taken over from {}: stale, no owner event since {idle}\n", previous.owner), format!("taken over by {by} from {}: stale, no owner event since {idle}", previous.owner))
        }
        Taken::Refused(holder) => return Err(format!("{shown} {OWNED} {}", holder.owner)),
    };
    lifecycle::record(context.database, &[Moved::held(&held, cause)], Timestamp::from_unix_seconds(context.now))?;
    Ok(printed)
}

/// Every claim that still stands, with its standing; a void claim holds nothing, so none shows.
pub fn standing_claims(context: &Context) -> Result<Vec<(Claim, Standing)>, String> {
    Ok(every_claim(context)?.into_iter().filter(|(_, standing)| !matches!(standing, Standing::Void(_))).collect())
}

/// Every claim row with its standing, void ones included: the rows `threads release` can still clear.
fn every_claim(context: &Context) -> Result<Vec<(Claim, Standing)>, String> {
    let db = ledger::open(context.database, false)?;
    let all = ledger::claims(&db)?;
    let present: BTreeSet<String> = all.iter().map(Claim::key).collect();
    let claims: Vec<Claim> = all.into_iter().filter(|claim| context.no_native.is_none() || CATALOG.parse_ref(&claim.item).is_ok_and(|(repo, _)| !CATALOG.is_native(&repo))).collect();
    let github = context.issues.read(&github_items(claims.iter().map(|claim| claim.item.as_str())));
    let standings: Vec<(Claim, Standing)> = claims
        .into_iter()
        .map(|claim| {
            let standing = claim.standing(issue_of(&db, &claim.item, &github), context.alive.as_ref(), context.now);
            (claim, standing)
        })
        .collect();
    if !context.replica {
        lifecycle::settle(context.database, &standings, &present, Timestamp::from_unix_seconds(context.now))?;
    }
    Ok(standings)
}

/// The claim rows on the named items and those the `--owner`s hold, as JSON `{item, owner}` rows, whatever their standing.
fn holder(parsed: &Args, context: &Context) -> Result<String, String> {
    if (parsed.positional.is_empty() && parsed.owners.is_empty()) || parsed.by.is_some() || parsed.eta.is_some() || parsed.json || parsed.all || parsed.box_text.is_some() || !parsed.parents.is_empty() {
        return Err("usage: threads holder [REPO#N ...] [--owner NAME ...]".into());
    }
    let items = parsed.positional.iter().map(|id| CATALOG.parse_ref(id).map(|(repo, number)| format!("{repo}#{number}"))).collect::<Result<Vec<_>, _>>()?;
    if !context.database.exists() {
        return Ok("[]\n".to_owned());
    }
    let db = ledger::open(context.database, false)?;
    let rows: Vec<_> = ledger::holders(&db, &items, &parsed.owners)?
        .into_iter()
        .filter(|(key, _)| context.no_native.is_none() || CATALOG.parse_ref(&split_key(key).0).is_ok_and(|(repo, _)| !CATALOG.is_native(&repo)))
        .map(|(key, owner)| {
            let (item, slug) = split_key(&key);
            json!({"item": item, "box": slug, "owner": owner})
        })
        .collect();
    Ok(format!("{}\n", serde_json::Value::Array(rows)))
}

fn list(parsed: &Args, context: &Context) -> Result<String, String> {
    if !parsed.positional.is_empty() || parsed.by.is_some() || parsed.eta.is_some() || parsed.box_text.is_some() || !parsed.owners.is_empty() || !parsed.parents.is_empty() || (parsed.all && !parsed.json) {
        return Err("usage: threads list [--json [--all]]".into());
    }
    let claims = if parsed.all { every_claim(context)? } else { standing_claims(context)? };
    let clock = |claim: &Claim| (context.now - claim.since).max(0) / 60;
    if parsed.json {
        let rows: Vec<_> = claims
            .iter()
            .map(|(claim, standing)| {
                json!({"item": claim.item, "owner": claim.owner, "box": claim.slug, "since": Timestamp::from_unix_seconds(claim.since).iso(), "clock_min": clock(claim), "eta_min": claim.eta_min, "standing": standing.label()})
            })
            .collect();
        return Ok(format!("{}\n", serde_json::Value::Array(rows)));
    }
    let mut rows = vec![["item", "owner", "clock", "eta", "flag", "box"].map(str::to_owned)];
    for (claim, standing) in &claims {
        let flag = match (standing, claim.eta_min) {
            (Standing::Stale { .. }, _) => "STALE",
            (_, Some(eta)) if clock(claim) as f64 > *OVERDUE_RATIO * eta as f64 => "OVERDUE",
            _ => "-",
        };
        let eta = claim.eta_min.map_or_else(|| "-".to_owned(), |eta| format!("{eta}m"));
        rows.push([claim.item.clone(), claim.owner.clone(), format!("{}m", clock(claim)), eta, flag.to_owned(), claim.slug.clone().unwrap_or_else(|| "-".to_owned())]);
    }
    let widths: Vec<usize> = (0..6).map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0)).collect();
    Ok(rows.iter().map(|row| format!("{}\n", row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end())).collect())
}
