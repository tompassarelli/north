//! The `agents usage` table: one line per account with its windows, billing, reset availability, burn and plan.
use std::collections::BTreeSet;

use rusqlite::Connection;

use super::store::Row;
use super::{Places, burn, costs, py, settings, web};
use crate::foundation::clock::Timestamp;

const NO_SNAPSHOT: &str = "agents usage: no snapshot yet; run `agents usage --refresh`";
const RESET_NAME: &str = "cedar_ember";

pub(super) fn render(db: &Connection, places: &Places, now: Timestamp) -> Result<(String, u8), String> {
    let rows = super::readings(db, &places.policy)?;
    if rows.is_empty() {
        return Ok((format!("{NO_SNAPSHOT}\n{}", costs::lines(places, now)?.join("\n")), 1));
    }
    let accounts: BTreeSet<(Option<&str>, Option<&str>)> = rows.iter().map(|row| (row.text("provider"), row.text("account"))).collect();
    let mut lines = Vec::new();
    for (provider, account) in accounts {
        let mine: Vec<&Row> = rows.iter().filter(|row| (row.text("provider"), row.text("account")) == (provider, account)).collect();
        lines.push(account_line(db, places, (provider, account), &mine, now)?);
    }
    lines.extend(costs::lines(places, now)?);
    Ok((lines.join("\n"), 0))
}

fn shown(value: Option<&str>) -> &str {
    value.unwrap_or("None")
}

fn account_line(db: &Connection, places: &Places, (provider, account): (Option<&str>, Option<&str>), mine: &[&Row], now: Timestamp) -> Result<String, String> {
    let mut age = f64::INFINITY;
    for row in mine {
        age = age.min(row.age_minutes(now)?.ok_or("a reading has no time")?);
    }
    let windows: Vec<String> = mine
        .iter()
        .filter(|row| row.text("kind") == Some("window"))
        .filter_map(|row| Some((row, row.number("used_pct")?)))
        .map(|(row, used)| Ok(format!("{} {used:.0}% used, resets in {}", shown(row.text("name")), py::until(row.text("resets_at"), now)?)))
        .collect::<Result<_, String>>()?;
    let claude = provider == Some("claude");
    let reset_row = mine.iter().find(|row| claude && row.text("name") == Some(RESET_NAME) && matches!(row.text("kind"), Some("billing" | "grant")));
    let granted = reset_row.and_then(|row| row.number("amount").map(|amount| (amount, row.text("source"))));
    let resets = match granted {
        Some((amount, source)) => format!("resets={} ({})", py::g(amount), shown(source)),
        None => match if claude { web::resets(&places.state, account, now)? } else { None } {
            Some(found) => reset_kinds(&found),
            None => format!("resets=unknown ({})", if claude { "no web reset reading for this account" } else { "provider adapter has no reset availability" }),
        },
    };
    let billing: Vec<String> = mine
        .iter()
        .filter(|row| matches!(row.text("kind"), Some("billing" | "grant")))
        .filter(|row| !(claude && row.text("name") == Some(RESET_NAME)) && row.text("name") != Some("plan") && !row.text("name").is_some_and(|name| name.starts_with("cedar_ember/")))
        .filter_map(|row| Some(format!("{}={}", shown(row.text("name")), py::g(row.number("amount")?))))
        .collect();
    let mut latest_first = mine.to_vec();
    latest_first.sort_by(|a, b| b.text("ts").cmp(&a.text("ts")));
    let observed = latest_first.iter().find(|row| row.text("kind") == Some("billing") && row.text("name") == Some("plan")).and_then(|row| row.text("source"));
    let stale = if age > settings::stale_minutes() { ", STALE" } else { "" };
    let billing = if billing.is_empty() { String::new() } else { format!(" | {}", billing.join(", ")) };
    Ok(format!(
        "{} {} ({age:.0} min ago{stale}): {}{billing} | {resets} | {}{}",
        shown(provider),
        shown(account),
        windows.join("; "),
        burn::describe(db, mine, now)?,
        settings::plan_text(&places.policy, shown(provider), account, observed)
    ))
}

fn reset_kinds(found: &web::Resets) -> String {
    let mut grants: Vec<&web::Grant> = found.grants.iter().collect();
    grants.sort_by_key(|grant| grant.name != "cedar_ember/full");
    let kinds: Vec<String> = grants
        .iter()
        .map(|grant| {
            let label = if grant.name.ends_with("/full") { "full" } else { "5h" };
            format!("{label}={}{}", py::g(grant.amount), grant.expires.as_ref().map_or(String::new(), |expires| format!(" (expires {expires})")))
        })
        .collect();
    format!("resets {} ({})", kinds.join(" "), found.source)
}
