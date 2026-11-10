//! The API section of the default `north fleet usage` table: one aligned row per pooled organization in the order launches spend
//! them (soonest expiring grant with credit left first), and one footer line saying which hosts' spend it was charged with.
use super::Places;
use super::plain::{self, grid};
use crate::credit::{self, Reading, Standing};
use crate::foundation::clock::Timestamp;

const HEADER: [&str; 5] = ["API", "grant", "expires", "spent", "left"];
const EXHAUSTED: &str = "exhausted";
const UNREAD: &str = "—";

fn order(org: &Standing) -> (u8, &str) {
    match &org.balance {
        None => (3, ""),
        Some(_) if org.exhausted => (2, ""),
        Some(balance) => (u8::from(org.left <= 0.0), balance.grant.expires.as_str()),
    }
}

pub(super) fn lines(places: &Places, now: Timestamp) -> Vec<String> {
    let credits = credit::standing(&places.policy, &places.state, now);
    if credits.orgs.is_empty() {
        return Vec::new();
    }
    let mut out = grid(&rows(credits.orgs, credits.spend.is_ok()), 0);
    out.push(footer(&credits.spend, &credit::peers(&places.policy), now.unix_millis()));
    out
}

fn rows(mut orgs: Vec<Standing>, read: bool) -> Vec<Vec<String>> {
    orgs.sort_by(|a, b| order(a).cmp(&order(b)));
    let mut header: Vec<String> = HEADER.map(str::to_owned).to_vec();
    let mut table: Vec<Vec<String>> = orgs.iter().map(|org| row(org, read)).collect();
    if table.iter().any(|row| row.len() > HEADER.len()) {
        header.push("then".to_owned());
    }
    table.insert(0, header);
    table
}

fn row(org: &Standing, read: bool) -> Vec<String> {
    let Some(balance) = &org.balance else { return vec![org.name.clone(), "none".to_owned(), UNREAD.to_owned(), UNREAD.to_owned(), UNREAD.to_owned()] };
    let money = |usd: f64| format!("${usd:.2}");
    let (spent, left) = match (read, org.exhausted) {
        (false, _) => (UNREAD.to_owned(), UNREAD.to_owned()),
        (true, true) => (money(balance.spent), EXHAUSTED.to_owned()),
        (true, false) => (money(balance.spent), money(org.left)),
    };
    let mut cells = vec![org.name.clone(), money(balance.grant.usd), plain::day(&balance.grant.expires), spent, left];
    if !balance.later.is_empty() {
        cells.push(balance.later.iter().map(|grant| format!("{} {}", money(grant.usd), plain::day(&grant.expires))).collect::<Vec<_>>().join(", "));
    }
    cells
}

fn footer(spends: &Result<Reading, String>, peers: &[String], now_ms: i64) -> String {
    match spends {
        Ok(reading) => reading.footer(&crate::platform::host::name(), peers, now_ms),
        Err(error) => format!("API spend unread: {error}."),
    }
}

#[cfg(test)]
mod tests;
