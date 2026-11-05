//! Each Claude account's weekly use and how long it lasts, for callers that act on `north usage`'s outlook column.
use std::path::Path;

use rusqlite::Connection;

use super::burn::{self, Burn};
use super::py;
use super::store::Row;
use crate::foundation::clock::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Runway {
    Out,
    Empties { minutes: f64 },
    Lasts,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountOutlook {
    pub account: String,
    pub week_used: Option<f64>,
    pub runway: Runway,
}

pub fn claude_accounts(db: &Connection, policy: &Path, now: Timestamp) -> Result<Vec<AccountOutlook>, String> {
    let rows = super::readings(db, policy)?;
    let claude = |row: &&Row| row.text("provider") == Some("claude");
    let mut names: Vec<&str> = rows.iter().filter(claude).filter_map(|row| row.text("account")).collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .map(|account| {
            let mine: Vec<&Row> = rows.iter().filter(claude).filter(|row| row.text("account") == Some(account)).collect();
            let week = current(&mine, "weekly_all", now)?;
            let session = current(&mine, "session", now)?;
            Ok(AccountOutlook { account: account.to_owned(), week_used: week.map(|(used, _)| used), runway: runway(db, &mine, week, session, now)? })
        })
        .collect()
}

fn runway(db: &Connection, mine: &[&Row], week: Option<(f64, Timestamp)>, session: Option<(f64, Timestamp)>, now: Timestamp) -> Result<Runway, String> {
    if week.into_iter().chain(session).any(|(used, _)| used >= 100.0) {
        return Ok(Runway::Out);
    }
    Ok(match burn::assess(db, mine, now)? {
        Burn::Unknown(_) => Runway::Unknown,
        Burn::Lasts { .. } => Runway::Lasts,
        Burn::Empties { at, .. } => Runway::Empties { minutes: py::minutes_precise(now, at).max(0.0) },
    })
}

fn current(mine: &[&Row], name: &str, now: Timestamp) -> Result<Option<(f64, Timestamp)>, String> {
    let Some(row) = mine.iter().find(|row| row.text("kind") == Some("window") && row.text("name") == Some(name)) else { return Ok(None) };
    let (Some(used), Some(resets_at)) = (row.number("used_pct"), row.text("resets_at").filter(|text| !text.is_empty())) else { return Ok(None) };
    let reset = py::parse(resets_at)?;
    Ok((reset > now).then_some((used, reset)))
}
