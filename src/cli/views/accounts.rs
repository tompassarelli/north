use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

pub struct Accounts {
    pub text: String,
    pub json: Value,
}

const USAGE: &str = "usage: north accounts [--json] | north accounts reset [--dry-run]";

pub fn run(arguments: &[String]) -> u8 {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        [verb, rest @ ..] if verb == "reset" => return reset(rest),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match super::Snapshot::new().accounts() {
        Ok(snapshot) => {
            if let Some(accounts) = &snapshot.accounts {
                if json {
                    println!("{}", accounts.json);
                } else {
                    print!("{}", accounts.text);
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
    let session = fleet.session("proxy").or_else(|_| fleet.session("coordinator")).map_err(|error| error.user_message())?;
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

pub fn read(at: Timestamp) -> Result<Accounts, String> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot)?;
    let places = crate::usage::Places::from_env();
    let usage = crate::usage::detail_text(&db, &places, at)?;
    let readings = crate::usage::json_readings(&db, at)?;
    let plan = crate::pacing::placement::case::plan(&crate::cli::fleet::roster().map_err(|error| error.user_message())?)?;
    let credit = north_core::credit::standing(&crate::platform::orchestration::path(), &crate::watch::state_home(), at).orgs;
    let runs = crate::ledger::runs_inside("24h").map_err(|error| error.user_message())?;
    let yield_line = crate::flow::usage_line();
    let mut text = format!("Accounts at {}\n\n{usage}\n\nPlan order:\n", at.iso());
    for (index, (line, reason)) in plan.iter().enumerate() {
        text.push_str(&format!("  {}. {line} [{reason}]\n", index + 1));
    }
    text.push_str("\nApi credit:\n");
    for org in &credit {
        text.push_str(&format!("  {}\n", org.line()));
    }
    text.push_str(&format!("\nRuns, last 24h: {}\n", runs.len()));
    for (account, (count, usd)) in spend(&runs) {
        text.push_str(&format!("  {account}: {count}, ${usd:.2}\n"));
    }
    text.push_str(&format!("\n{yield_line}\n"));
    Ok(Accounts {
        text,
        json: json!({
            "at": at.iso(),
            "usage": readings,
            "plan": plan.iter().map(|(line, reason)| json!({"line": line, "reason": reason})).collect::<Vec<_>>(),
            "api_credit": credit.iter().map(|org| json!({"account": format!("api:{}", org.name), "left_usd": org.left, "estimate_usd": org.estimated, "exhausted": org.exhausted, "figure": org.figure()})).collect::<Vec<_>>(),
            "runs_24h": runs.iter().map(|run| json!({"account": run.account, "model": run.model, "effort": run.effort, "outcome": run.outcome, "usd": run.usd})).collect::<Vec<_>>(),
            "usage_yield": yield_line,
        }),
    })
}

fn spend(runs: &[crate::ledger::WindowRun]) -> BTreeMap<String, (usize, f64)> {
    let mut found: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for run in runs {
        let account = if run.account.is_empty() { "unrecorded".to_owned() } else { run.account.clone() };
        let entry = found.entry(account).or_default();
        entry.0 += 1;
        entry.1 += run.usd;
    }
    found
}
