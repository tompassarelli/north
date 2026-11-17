use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::usage::{ApiGrant, Placement, State};

pub struct Accounts {
    pub text: String,
    pub json: Option<Value>,
}

const USAGE: &str = "usage: north accounts [--json | --emails] | north accounts reset [--dry-run]";

pub fn run(arguments: &[String]) -> u8 {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        [flag] if flag == "--emails" => return emails(),
        [verb, rest @ ..] if verb == "reset" => return reset(rest),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match super::Snapshot::new().accounts(json) {
        Ok(snapshot) => {
            if let Some(accounts) = &snapshot.accounts {
                match &accounts.json {
                    Some(detail) => println!("{detail}"),
                    None => print!("{}", accounts.text),
                }
            }
            0
        }
        Err(why) => {
            eprintln!("north accounts: {why}");
            1
        }
    }
}

/// Prints the sign-in email each signed-in subscription profile records for itself, one per line, the names `claude EMAIL` takes.
fn emails() -> u8 {
    match emails_run() {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("north accounts --emails: {why}");
            1
        }
    }
}

fn emails_run() -> Result<(), String> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let roster = crate::cli::fleet::roster().map_err(|error| error.user_message())?;
    let emails: BTreeSet<String> = roster
        .accounts
        .iter()
        .filter(|account| account.provider.label() == "claude" && account.api.is_none())
        .filter_map(|account| north_providers::providers::claude::sign_in_email(&account.home, &home))
        .collect();
    for email in emails {
        println!("{email}");
    }
    Ok(())
}

fn reset(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match reset_run(dry_run) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("north accounts reset: {why}");
            1
        }
    }
}

/// Checks the coordinator's account for an offered usage reset and records the check in the fleet's
/// reset log. No browser path performs it: Anthropic's Consumer Terms forbid automated access to
/// claude.ai and no provider reset command or API exists (docs/rationale/usage-resets.md).
fn reset_run(dry_run: bool) -> Result<(), String> {
    let at = Timestamp::now();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot)?;
    let readings = crate::usage::json_readings(&db, at)?;
    let fleet = crate::cli::fleet::open_fleet().map_err(|error| error.user_message())?;
    let session = fleet.session("coordinator").map_err(|error| error.user_message())?;
    let account = session.account;
    let used = fable_used(&readings, &account).ok_or_else(|| format!("no Fable reading for {account}"))?;
    let places = crate::usage::Places::from_env();
    let offered = crate::usage::reset::offered(&places.state, &account, at)?;
    let outcome = crate::usage::reset::assess(used, offered.as_ref());
    let log = places.state.join("north/usage/resets.jsonl");
    if !dry_run {
        crate::usage::reset::record(&log, &account, at, used, used, offered.as_ref(), &outcome)?;
    }
    let grants = offered.as_ref().map_or_else(|| "resets none".to_owned(), |offered| {
        format!("resets full={:.0} 5h={:.0}{}", offered.full, offered.five_hour, offered.expires.as_ref().map_or(String::new(), |expires| format!(" expires {expires}")))
    });
    println!(
        "north accounts reset: {account} Fable {used:.1}% (floor {:.0}%), {grants}; {}: {}; {}",
        crate::usage::decide::SUBSCRIPTION_FLOOR.0,
        outcome.label(),
        outcome.reason(),
        if dry_run { "log not written (--dry-run)".to_owned() } else { format!("logged to {}", log.display()) }
    );
    Ok(())
}

fn fable_used(readings: &[Value], account: &str) -> Option<f64> {
    latest_reading(readings, account, "weekly_scoped/Fable").or_else(|| latest_reading(readings, account, "weekly_all")).and_then(|row| row["used_pct"].as_f64())
}

fn latest_reading<'a>(readings: &'a [Value], account: &str, name: &str) -> Option<&'a Value> {
    readings
        .iter()
        .filter(|row| row["provider"] == "claude" && row["account"] == account && row["kind"] == "window" && row["name"] == name && row["used_pct"].is_number())
        .max_by_key(|row| row["ts"].as_str().unwrap_or_default())
}

/// The account rows from the cached readings alone, with the machine detail (every reading, the placement order, and runs) when `detail` asks for it.
pub fn read(at: Timestamp, detail: bool) -> Result<Accounts, String> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let db = crate::usage::open_ledger(&home, None, &|| {})?;
    let places = crate::usage::Places::from_env();
    let credit = north_pace::credit::standing(&crate::platform::orchestration::path(), &crate::platform::records_mirror::state_home(), at).orgs;
    let placement: BTreeMap<String, Placement> = BTreeMap::new();
    let held = crate::spend::hold::at(&crate::platform::orchestration::path());
    let grants: Vec<ApiGrant> = credit.iter().map(|org| grant(org, held.as_ref())).collect();
    let view = crate::usage::SummaryView { db: &db, places: &places, now: at, offset_seconds: crate::foundation::clock::local_offset_seconds(), placement: &placement, grants: &grants };
    let text = crate::usage::summary(&view)?;
    let json = if detail { Some(detailed(&db, at, &credit)?) } else { None };
    Ok(Accounts { text, json })
}


fn grant(org: &north_pace::credit::Standing, held: Option<&crate::spend::hold::Hold>) -> ApiGrant {
    let expires = org.balance.as_ref().map(|balance| balance.grant.expires.clone());
    let (state, reason) = if let Some(held) = held {
        (State::Held, held.line(&format!("api:{}", org.name)))
    } else if org.exhausted {
        (State::Out, "the provider refused a turn for credit".to_owned())
    } else if org.left < 0.005 {
        (State::Out, "no credit left".to_owned())
    } else {
        (State::Open, String::new())
    };
    ApiGrant { name: org.name.clone(), left_usd: org.left, expires, state, reason }
}

fn detailed(db: &rusqlite::Connection, at: Timestamp, credit: &[north_pace::credit::Standing]) -> Result<Value, String> {
    let readings = crate::usage::json_readings(db, at)?;
    let runs = crate::ledger::runs_inside("24h").map_err(|error| error.user_message())?;
    Ok(json!({
        "at": at.iso(),
        "usage": readings,
        "api_credit": credit.iter().map(|org| json!({"account": format!("api:{}", org.name), "left_usd": org.left, "estimate_usd": org.estimated, "exhausted": org.exhausted, "figure": org.figure()})).collect::<Vec<_>>(),
        "runs_24h": runs.iter().map(|run| json!({"account": run.account, "model": run.model, "effort": run.effort, "outcome": run.outcome, "usd": run.usd})).collect::<Vec<_>>(),
    }))
}
