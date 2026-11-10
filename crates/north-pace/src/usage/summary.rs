//! `north accounts`: one aligned row per Claude account and per api grant, read from the cached usage table alone.
use std::collections::BTreeMap;

use rusqlite::Connection;

use super::plain::{self, spaced};
use super::store::Row;
use super::table::{current, plan_label};
use super::Places;
use crate::foundation::clock::Timestamp;

const NONE: &str = "—";
const GAP: &str = "  ";
const WIDTH: usize = 100;
const ELLIPSIS: char = '…';
const CLAUDE_HEADER: [&str; 8] = ["account", "plan", "state", "5h left", "5h resets", "week left", "week resets", "why"];
const GRANT_HEADER: [&str; 5] = ["api grant", "state", "left", "expires", "why"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Open,
    Out,
    Held,
}

impl State {
    fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Out => "out",
            Self::Held => "held",
        }
    }
}

/// What placement decided about an account and, when it is not open, why.
pub struct Placement {
    pub state: State,
    pub reason: String,
}

/// An api organization's credit as the account view shows it.
pub struct ApiGrant {
    pub name: String,
    pub left_usd: f64,
    pub expires: Option<String>,
    pub state: State,
    pub reason: String,
}

pub struct View<'a> {
    pub db: &'a Connection,
    pub places: &'a Places,
    pub now: Timestamp,
    pub offset_seconds: i64,
    pub placement: &'a BTreeMap<String, Placement>,
    pub grants: &'a [ApiGrant],
}

pub fn render(view: &View) -> Result<String, String> {
    let rows = super::readings(view.db, &view.places.policy)?;
    let mut accounts: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows.iter().filter(|row| row.text("provider") == Some("claude")) {
        accounts.entry(row.text("account").unwrap_or_default()).or_default().push(row);
    }
    for account in view.placement.keys() {
        accounts.entry(account.as_str()).or_default();
    }
    let mut table = Vec::new();
    for (account, mine) in &accounts {
        table.push(claude_row(view, account, mine)?);
    }
    let mut blocks = Vec::new();
    if !table.is_empty() {
        table.insert(0, CLAUDE_HEADER.map(str::to_owned).to_vec());
        blocks.push(fitted(spaced(&table, 0, GAP)));
    }
    if !view.grants.is_empty() {
        let mut table = vec![GRANT_HEADER.map(str::to_owned).to_vec()];
        table.extend(view.grants.iter().map(grant_row));
        blocks.push(fitted(spaced(&table, 0, GAP)));
    }
    if blocks.is_empty() {
        return Ok("No usage readings yet; run `north fleet usage --refresh`.".to_owned());
    }
    Ok(blocks.iter().map(|block| block.join("\n")).collect::<Vec<_>>().join("\n\n") + "\n")
}

fn claude_row(view: &View, account: &str, mine: &[&Row]) -> Result<Vec<String>, String> {
    let (now, offset) = (view.now, view.offset_seconds);
    let session = current(mine, "session", now)?;
    let week = current(mine, "weekly_all", now)?;
    let left = |window: Option<(f64, Timestamp)>| window.map_or(NONE.to_owned(), |(used, _)| format!("{:.0}%", (100.0 - used).clamp(0.0, 100.0)));
    let resets = |window: Option<(f64, Timestamp)>| window.map_or(NONE.to_owned(), |(_, reset)| plain::clock_near(reset, now, offset));
    let full: Vec<&str> = [("5h", session), ("week", week)].into_iter().filter(|(_, window)| window.is_some_and(|(used, _)| used >= 100.0)).map(|(name, _)| name).collect();
    let (state, reason) = match view.placement.get(account) {
        Some(placement) if placement.state == State::Out && !full.is_empty() => (State::Out, format!("{} full", full.join(" and "))),
        Some(placement) => (placement.state, placement.reason.clone()),
        None if full.is_empty() => (State::Open, String::new()),
        None => (State::Out, format!("{} full", full.join(" and "))),
    };
    let reason = if state == State::Open { String::new() } else { brief(&reason) };
    Ok(vec![account.to_owned(), plan_label(&view.places.policy, account, mine), state.word().to_owned(), left(session), resets(session), left(week), resets(week), reason])
}

fn grant_row(grant: &ApiGrant) -> Vec<String> {
    let expires = grant.expires.as_deref().map_or(NONE.to_owned(), plain::day);
    let reason = if grant.state == State::Open { String::new() } else { brief(&grant.reason) };
    vec![grant.name.clone(), grant.state.word().to_owned(), format!("${:.2}", grant.left_usd.max(0.0)), expires, reason]
}

/// A reason without its parenthetical detail, which `--json` keeps.
fn brief(reason: &str) -> String {
    reason.split_once(" (").map_or(reason, |(head, _)| head).to_owned()
}

/// Cuts the last column of every line to the screen width, so a long reason never wraps.
fn fitted(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|line| if line.chars().count() <= WIDTH { line } else { line.chars().take(WIDTH - 1).chain(std::iter::once(ELLIPSIS)).collect() }).collect()
}
