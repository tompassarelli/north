use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::Connection;

use super::super::decide::world::{
    Account, Credit, Host, Label, LimitEvent, Percent, Provider, Rate, Rates, Reading, RowId,
    Session, Settings, Source, Trip, Unix, Window, World,
};
use super::super::store::Row;
use crate::foundation::clock::Timestamp;

const POOLED: Rate = Rate::Pooled {
    points_per_usd: 1.0,
    n: 0,
};

pub struct Parts {
    pub now: Unix,
    pub accounts: Vec<Account>,
    pub readings: Vec<Reading>,
    pub limits: Vec<LimitEvent>,
    pub sessions: Vec<Session>,
    pub tripped: Vec<Trip>,
    pub credit: Option<Credit>,
    pub settings: Settings,
}

impl Parts {
    pub fn world(&self) -> World<'_> {
        World {
            now: self.now,
            accounts: &self.accounts,
            readings: &self.readings,
            limits: &self.limits,
            sessions: &self.sessions,
            tripped: &self.tripped,
            credit: self.credit,
            settings: &self.settings,
        }
    }
}

pub fn parts(
    db: &Connection,
    policy: &Path,
    host: &str,
    now: Unix,
    sessions: Vec<Session>,
) -> Result<Parts, String> {
    let rows = super::super::readings(db, policy)?;
    let claude: Vec<&Row> = rows
        .iter()
        .filter(|row| row.text("provider") == Some("claude"))
        .collect();
    let mut readings = Vec::new();
    for (index, row) in claude.iter().enumerate() {
        readings.extend(reading(row, index as i64));
    }
    Ok(Parts {
        now,
        accounts: accounts(&claude, host),
        readings,
        limits: claude.iter().filter_map(|row| limit(row)).collect(),
        sessions,
        tripped: Vec::new(),
        credit: None,
        settings: settings(policy),
    })
}

fn accounts(rows: &[&Row], host: &str) -> Vec<Account> {
    let labels: BTreeSet<&str> = rows.iter().filter_map(|row| row.text("account")).collect();
    let extra: BTreeMap<&str, bool> = rows
        .iter()
        .filter(|row| {
            row.text("kind") == Some("billing") && row.text("name") == Some("extra_usage")
        })
        .filter_map(|row| Some((row.text("account")?, row.number("amount")? > 0.0)))
        .collect();
    labels
        .into_iter()
        .map(|label| Account {
            provider: Provider::Claude,
            label: Label(label.to_owned()),
            weight: 1.0,
            signed_in: BTreeSet::from([Host(host.to_owned())]),
            extra_usage_enabled: extra.get(label).copied(),
            extra_usage_spend: None,
            rates: Rates {
                five_hour: POOLED,
                week: POOLED,
            },
        })
        .collect()
}

fn reading(row: &Row, index: i64) -> Option<Reading> {
    if row.text("kind") != Some("window") {
        return None;
    }
    Some(Reading {
        row: RowId(index),
        account: Label(row.text("account")?.to_owned()),
        window: window(row.text("name")?)?,
        used: Percent(row.number("used_pct")?),
        resets_at: row.text("resets_at").and_then(unix),
        read_at: row.text("ts").and_then(unix)?,
        source: source(row.text("source").unwrap_or_default()),
    })
}

fn limit(row: &Row) -> Option<LimitEvent> {
    if row.text("kind") != Some("limit") {
        return None;
    }
    Some(LimitEvent {
        account: Label(row.text("account")?.to_owned()),
        window: window(row.text("name")?)?,
        resets_at: row.text("resets_at").and_then(unix),
        at: row.text("ts").and_then(unix)?,
    })
}

fn window(name: &str) -> Option<Window> {
    match name {
        "five_hour" | "session" => Some(Window::FiveHour),
        "seven_day" | "weekly_all" => Some(Window::Week),
        scoped => {
            super::launches::family(scoped.strip_prefix("weekly_scoped/")?).map(Window::Scoped)
        }
    }
}

fn source(text: &str) -> Source {
    let text = text.to_ascii_lowercase();
    if text.contains("stream") {
        Source::Stream
    } else if text.contains("header") {
        Source::Header
    } else if text.contains("web") {
        Source::Web
    } else {
        Source::Cli
    }
}

fn unix(text: &str) -> Option<Unix> {
    Timestamp::parse_rfc3339(text).map(|at| Unix(at.unix_seconds()))
}

fn settings(policy: &Path) -> Settings {
    let reserve = std::fs::read_to_string(policy)
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|document| {
            let table = document.get("usage")?.get("reserve")?.as_table_like()?;
            let percent = |name: &str| {
                table.get(name).and_then(|item| {
                    item.as_float()
                        .or_else(|| item.as_integer().map(|value| value as f64))
                })
            };
            percent("claude").map(|percent| BTreeMap::from([(Provider::Claude, Percent(percent))]))
        })
        .unwrap_or_default();
    Settings {
        reserve,
        crossings: Vec::new(),
        clears: Vec::new(),
    }
}
