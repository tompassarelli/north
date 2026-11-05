//! The default `north usage` table: one aligned row per Claude account with its 5-hour and weekly windows, the
//! weekly reset in local time and an outlook, one line per other provider, and this week's operating cost.
use std::collections::BTreeMap;

use rusqlite::Connection;

use super::burn::{self, Burn};
use super::settings::Plan;
use super::store::Row;
use super::{Places, costs, detail, plain, py, settings, web};
use crate::foundation::clock::Timestamp;

const NO_SNAPSHOT: &str = "No usage readings yet; run `north usage --refresh`.";
const HEADER: [&str; 5] = ["Claude", "5 hours", "week", "week resets", "outlook"];
const GAP: &str = "   ";
const NONE: &str = "—";

pub(super) struct View<'a> {
    pub db: &'a Connection,
    pub places: &'a Places,
    pub now: Timestamp,
    pub offset_seconds: i64,
}

pub(super) fn render(view: &View) -> Result<(String, u8), String> {
    let rows = super::readings(view.db, &view.places.policy)?;
    let mut accounts: BTreeMap<(&str, &str), Vec<&Row>> = BTreeMap::new();
    for row in &rows {
        accounts.entry((row.text("provider").unwrap_or_default(), row.text("account").unwrap_or_default())).or_default().push(row);
    }
    let (mut claude, mut lines) = (Vec::new(), Vec::new());
    for (&(provider, account), mine) in &accounts {
        if provider == "claude" {
            claude.push(claude_row(view, account, mine)?);
        } else {
            let several = accounts.keys().filter(|(other, _)| *other == provider).count() > 1;
            let label = format!("{}{}", capitalized(provider), if several { format!(" {account}") } else { String::new() });
            lines.push((label, other_line(view.now, mine)?));
        }
    }
    if let Some(costs) = costs::read(view.places, view.now)? {
        lines.push(("This week".to_owned(), cost_line(&costs)));
    }
    if !claude.is_empty() {
        claude.insert(0, HEADER.map(str::to_owned).to_vec());
    }
    let text = layout(&claude, &lines);
    Ok(if rows.is_empty() { (format!("{NO_SNAPSHOT}\n{text}").trim_end().to_owned(), 1) } else { (text, 0) })
}

fn layout(table: &[Vec<String>], lines: &[(String, String)]) -> String {
    let width = |cell: &String| cell.chars().count();
    let mut widths = vec![0; HEADER.len()];
    for row in table {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(width(cell));
        }
    }
    widths[0] = lines.iter().map(|(label, _)| width(label)).fold(widths[0], usize::max);
    let padded = |cell: &String, column: usize| format!("{cell}{}{GAP}", " ".repeat(widths[column] - width(cell)));
    let mut out: Vec<String> = table.iter().map(|row| row.iter().enumerate().map(|(column, cell)| padded(cell, column)).collect::<String>().trim_end().to_owned()).collect();
    if !table.is_empty() && !lines.is_empty() {
        out.push(String::new());
    }
    out.extend(lines.iter().map(|(label, rest)| format!("{}{rest}", padded(label, 0)).trim_end().to_owned()));
    out.join("\n")
}

fn claude_row(view: &View, account: &str, mine: &[&Row]) -> Result<Vec<String>, String> {
    let now = view.now;
    let session = current(mine, "session", now)?;
    let week = current(mine, "weekly_all", now)?;
    let plan = match settings::plan(&view.places.policy, "claude", Some(account), detail::observed_plan(mine)) {
        Some(Plan::Named { name, .. }) => name.strip_prefix("Claude ").unwrap_or(&name).to_owned(),
        Some(Plan::Key(key)) => key,
        None => String::new(),
    };
    let session_cell = session.map_or(NONE.to_owned(), |(used, reset)| format!("{} ({})", percent(used), plain::span(py::minutes_between(now, reset))));
    let mut week_cell = week.map_or(NONE.to_owned(), |(used, _)| if used >= 100.0 { "100% (full)".to_owned() } else { percent(used) });
    for row in mine {
        let Some(model) = row.text("name").and_then(|name| name.strip_prefix("weekly_scoped/")) else { continue };
        if let Some((used, _)) = current_row(row, now)?
            && used > 0.0
        {
            week_cell.push_str(&format!(", {model} {}", percent(used)));
        }
    }
    let resets = week.map_or(NONE.to_owned(), |(_, reset)| plain::clock(reset, view.offset_seconds));
    let mut outlook = vec![outlook(view, mine, session, week)?];
    outlook.extend(resets_left(view, account, mine)?);
    outlook.extend(billing(mine));
    let age = age_minutes(mine, now)?;
    if age > settings::stale_minutes() {
        outlook.push(plain::ago(age));
    }
    Ok(vec![format!("{account}  {plan}").trim_end().to_owned(), session_cell, week_cell, resets, outlook.join("; ")])
}

fn outlook(view: &View, mine: &[&Row], session: Option<(f64, Timestamp)>, week: Option<(f64, Timestamp)>) -> Result<String, String> {
    if week.is_some_and(|(used, _)| used >= 100.0) {
        return Ok("out until reset".to_owned());
    }
    if let Some((_, reset)) = session.filter(|(used, _)| *used >= 100.0) {
        return Ok(format!("out for {}", plain::span(py::minutes_between(view.now, reset))));
    }
    Ok(match burn::assess(view.db, mine, view.now)? {
        Burn::Unknown(_) => "no burn rate yet".to_owned(),
        Burn::Lasts { .. } => "lasts to reset".to_owned(),
        Burn::Empties { at, .. } => format!("runs out in {}", plain::span(py::minutes_between(view.now, at))),
    })
}

fn resets_left(view: &View, account: &str, mine: &[&Row]) -> Result<Option<String>, String> {
    let granted = mine.iter().find(|row| row.text("name") == Some(detail::RESET_NAME) && matches!(row.text("kind"), Some("billing" | "grant")));
    if let Some(amount) = granted.and_then(|row| row.number("amount")) {
        return Ok((amount > 0.0).then(|| format!("{} resets left", py::g(amount))));
    }
    let Some(found) = web::resets(&view.places.state, Some(account), view.now)? else { return Ok(None) };
    let mut grants: Vec<&web::Grant> = found.grants.iter().filter(|grant| grant.amount > 0.0).collect();
    grants.sort_by_key(|grant| !grant.name.ends_with("/full"));
    let kinds: Vec<String> = grants.iter().map(|grant| format!("{} {}", py::g(grant.amount), if grant.name.ends_with("/full") { "full" } else { "five-hour" })).collect();
    Ok((!kinds.is_empty()).then(|| format!("{} resets left", kinds.join(" and "))))
}

fn billing(mine: &[&Row]) -> Vec<String> {
    mine.iter()
        .filter(|row| matches!(row.text("kind"), Some("billing" | "grant")))
        .filter(|row| row.text("name").is_some_and(|name| name != "plan" && !name.starts_with(detail::RESET_NAME)))
        .filter_map(|row| Some((row.text("name")?, row.number("amount").filter(|amount| *amount != 0.0)?)))
        .map(|(name, amount)| format!("{} {}", name.replace('_', " "), py::g(amount)))
        .collect()
}

fn other_line(now: Timestamp, mine: &[&Row]) -> Result<String, String> {
    let mut periods: BTreeMap<i64, (String, Vec<String>)> = BTreeMap::new();
    for row in mine.iter().filter(|row| row.text("kind") == Some("window")) {
        let Some((used, _)) = current_row(row, now)? else { continue };
        let minutes = row.number("window_min").unwrap_or_default() as i64;
        if used == 0.0 && minutes < 10080 {
            continue;
        }
        let (label, suffix) = row.text("name").unwrap_or_default().split_once('/').map_or((None, ""), |(label, suffix)| (Some(label), suffix));
        let entry = periods.entry(minutes).or_insert_with(|| (period(minutes, suffix), Vec::new()));
        entry.1.push(label.map_or_else(|| percent(used), |label| format!("{} ({label})", percent(used))));
    }
    let mut parts: Vec<String> = periods.into_values().map(|(name, cells)| format!("{name} {}", cells.join(", "))).collect();
    if parts.is_empty() {
        parts.push("no current window".to_owned());
    }
    parts.extend(billing(mine));
    Ok(format!("{}{GAP}{}", parts.join("; "), plain::ago(age_minutes(mine, now)?)))
}

fn cost_line(costs: &costs::Costs) -> String {
    let services = if costs.current.is_empty() { String::new() } else { format!(" ({})", costs.current.join(", ")) };
    format!("${:.2}: ${:.2} fixed, ${:.2} metered{services}", costs.fixed + costs.metered, costs.fixed, costs.metered)
}

fn period(minutes: i64, suffix: &str) -> String {
    match minutes {
        300 => "5 hours".to_owned(),
        1440 => "day".to_owned(),
        10080 => "week".to_owned(),
        _ if !suffix.is_empty() => suffix.to_owned(),
        _ => plain::span(minutes),
    }
}

fn current(mine: &[&Row], name: &str, now: Timestamp) -> Result<Option<(f64, Timestamp)>, String> {
    match mine.iter().find(|row| row.text("kind") == Some("window") && row.text("name") == Some(name)) {
        Some(row) => current_row(row, now),
        None => Ok(None),
    }
}

fn current_row(row: &Row, now: Timestamp) -> Result<Option<(f64, Timestamp)>, String> {
    let (Some(used), Some(resets_at)) = (row.number("used_pct"), row.text("resets_at").filter(|text| !text.is_empty())) else { return Ok(None) };
    let reset = py::parse(resets_at)?;
    Ok((reset > now).then_some((used, reset)))
}

fn age_minutes(mine: &[&Row], now: Timestamp) -> Result<f64, String> {
    let mut age = f64::INFINITY;
    for row in mine {
        age = age.min(row.age_minutes(now)?.ok_or("a reading has no time")?);
    }
    Ok(age)
}

fn percent(used: f64) -> String {
    format!("{used:.0}%")
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| first.to_uppercase().chain(characters).collect())
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
