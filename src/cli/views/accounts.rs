use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

pub struct Accounts {
    pub text: String,
    pub json: Value,
}

const USAGE: &str = "usage: north accounts [--json]";

pub fn run(arguments: &[String]) -> u8 {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
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
