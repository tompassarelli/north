use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use toml_edit::{DocumentMut, Item};

use crate::foundation::clock::Timestamp;
use crate::usage::{py, settings};

const WITHIN_DAYS: i128 = 7;
const NANOS_PER_DAY: i128 = 86_400_000_000_000;
const DECISION: &str = "Blocked: decision for Tom: ";

pub(super) fn notify(lines: &[String], state: &Path, send: &dyn Fn(&str, &str)) -> Result<(), String> {
    let sent = state.join("agents/promo-notified");
    for line in lines {
        let account = line.split(" promotion").next().unwrap_or_default().trim_start_matches(DECISION);
        let ends = line.split(" ends ").nth(1).and_then(|rest| rest.split_whitespace().next()).unwrap_or_default();
        let stamp = sent.join(format!("{account}-{ends}"));
        if stamp.exists() {
            continue;
        }
        fs::create_dir_all(&sent).map_err(|error| error.to_string())?;
        send("Plan promotion ending", line);
        fs::write(&stamp, format!("{line}\n")).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) fn lines(catalog: &DocumentMut, db: &Connection, now: Timestamp) -> Result<Vec<String>, String> {
    let accounts: Vec<(&str, &Item)> = catalog.get("accounts").and_then(Item::as_table_like).into_iter().flat_map(|table| table.iter()).collect();
    let full: BTreeMap<&str, f64> = accounts
        .iter()
        .filter_map(|(provider, account)| {
            let price = number(account.get("monthly_price")?)?;
            let total = account.get("monthly_total").and_then(number).unwrap_or(price * account.get("account_count").and_then(number).unwrap_or(1.0));
            Some((*provider, total))
        })
        .collect();
    let spend: f64 = full.values().sum();
    let mut out = Vec::new();
    for (provider, account) in accounts {
        let Some(end) = account.get("promo_end").and_then(Item::as_datetime).and_then(|datetime| datetime.date) else { continue };
        let midnight = py::midnight(end.year, end.month, end.day)?;
        let left = (midnight - now.unix_nanos()).div_euclid(NANOS_PER_DAY);
        if !(0..=WITHIN_DAYS).contains(&left) {
            continue;
        }
        let counts = recent_runs(db, now)?;
        let runs = counts.get(provider).copied().unwrap_or(0);
        let total: usize = counts.values().sum();
        let run_share = if total == 0 { 0.0 } else { 100.0 * runs as f64 / total as f64 };
        let cost_share = 100.0 * full.get(provider).copied().unwrap_or(0.0) / if spend == 0.0 { 1.0 } else { spend };
        let verdict = if run_share >= cost_share { "earned its full price" } else { "did not earn its full price" };
        let confirmed = if account.get("promo_end_confirmed").and_then(Item::as_bool).unwrap_or(false) { "confirmed" } else { "unconfirmed" };
        out.push(format!(
            "{DECISION}{provider} promotion (USD {}/month) ends {end} ({left} days, {confirmed}); at USD {}/month it is {cost_share:.1}% of plan spend and ran {run_share:.1}% of runs in 30 days ({runs} of {total}): {verdict}; keep it at full price?",
            account.get("promo_price").map_or_else(|| "?".to_owned(), settings::number_text),
            account.get("monthly_price").map_or_else(|| "?".to_owned(), settings::number_text),
        ));
    }
    Ok(out)
}

fn number(item: &Item) -> Option<f64> {
    item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))
}

fn recent_runs(db: &Connection, now: Timestamp) -> Result<BTreeMap<&'static str, usize>, String> {
    let fail = |error: rusqlite::Error| error.to_string();
    let since = py::second(py::before(now, 30 * 86_400));
    let mut query = db.prepare("SELECT tier FROM runs WHERE started >= ?").map_err(fail)?;
    let tiers = query.query_map([since], |row| row.get::<_, Option<String>>(0)).map_err(fail)?;
    let mut counts = BTreeMap::new();
    for tier in tiers {
        *counts.entry(provider_of(&tier.map_err(fail)?.unwrap_or_default())).or_insert(0) += 1;
    }
    Ok(counts)
}

fn provider_of(tier: &str) -> &'static str {
    if tier.starts_with("gemini") {
        "gemini"
    } else if tier.starts_with("gpt-") {
        "codex"
    } else {
        "claude"
    }
}
