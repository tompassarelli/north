//! Each Claude account's weekly use and how long it lasts, for callers that act on `north fleet usage`'s outlook column.
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
    pub week_resets: Option<Timestamp>,
    pub five_hour_used: Option<f64>,
    pub five_hour_resets: Option<Timestamp>,
    /// The weekly window's use per hour over the last readings, in percentage points; None until two readings span it.
    pub burn_per_hour: Option<f64>,
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
            let burn = burn::assess(db, &mine, now)?;
            let burn_per_hour = match &burn {
                Burn::Lasts { rate, .. } | Burn::Empties { rate, .. } => Some(*rate),
                Burn::Unknown(_) => None,
            };
            Ok(AccountOutlook { account: account.to_owned(), week_used: week.map(|(used, _)| used), week_resets: week.map(|(_, reset)| reset), five_hour_used: session.map(|(used, _)| used), five_hour_resets: session.map(|(_, reset)| reset), burn_per_hour, runway: runway(&burn, week, session, now) })
        })
        .collect()
}

fn runway(burn: &Burn, week: Option<(f64, Timestamp)>, session: Option<(f64, Timestamp)>, now: Timestamp) -> Runway {
    if week.into_iter().chain(session).any(|(used, _)| used >= 100.0) {
        return Runway::Out;
    }
    match burn {
        Burn::Unknown(_) => Runway::Unknown,
        Burn::Lasts { .. } => Runway::Lasts,
        Burn::Empties { at, .. } => Runway::Empties { minutes: py::minutes_precise(now, *at).max(0.0) },
    }
}

fn current(mine: &[&Row], name: &str, now: Timestamp) -> Result<Option<(f64, Timestamp)>, String> {
    let Some(row) = mine.iter().find(|row| row.text("kind") == Some("window") && row.text("name") == Some(name)) else { return Ok(None) };
    let (Some(used), Some(resets_at)) = (row.number("used_pct"), row.text("resets_at").filter(|text| !text.is_empty())) else { return Ok(None) };
    let reset = py::parse(resets_at)?;
    Ok((reset > now).then_some((used, reset)))
}

/// A time as the weekday and 12-hour clock in the laptop's timezone, such as `Thu 9:00 AM`.
pub fn weekday_clock(at: Timestamp) -> String {
    super::plain::clock(at, crate::foundation::clock::local_offset_seconds())
}
