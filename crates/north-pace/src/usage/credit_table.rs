//! The API section of the default `north fleet usage` table: one aligned row per pooled organization in the order launches spend
//! them (soonest expiring grant with credit left first), and one footer line saying which hosts' spend it was charged with.
use super::Places;
use super::plain::{self, grid};
use crate::credit::{self, Balance, Reading};
use crate::foundation::clock::Timestamp;

const HEADER: [&str; 5] = ["API", "grant", "expires", "spent", "left"];
const EXHAUSTED: &str = "exhausted";
const UNREAD: &str = "—";

struct Entry {
    name: String,
    balance: Option<Balance>,
    marked: bool,
}

impl Entry {
    fn exhausted(&self) -> bool {
        self.marked || self.balance.as_ref().is_some_and(Balance::exhausted)
    }

    fn order(&self) -> (u8, &str) {
        match &self.balance {
            None => (2, ""),
            Some(balance) => (u8::from(self.exhausted()), balance.grant.expires.as_str()),
        }
    }
}

pub(super) fn lines(places: &Places, now: Timestamp) -> Vec<String> {
    let named = credit::orgs(&places.policy);
    if named.is_empty() {
        return Vec::new();
    }
    let grants = credit::grants(&places.policy);
    let spends = crate::router::pricing::PriceTable::load().map_err(|error| error.to_string()).and_then(|prices| credit::spends(&places.state.join("north/ledger.db"), &prices, now));
    let today = now.iso_date();
    let entries = named
        .into_iter()
        .map(|(name, id)| {
            let mine = spends.as_ref().ok().and_then(|reading| reading.orgs.get(&name).cloned()).unwrap_or_default();
            let balance = credit::balance(grants.get(&id).map_or(&[][..], Vec::as_slice), &mine, &today);
            Entry { name, balance, marked: places.state.join("north/api-exhausted").join(&id).exists() }
        })
        .collect();
    let mut out = grid(&rows(entries, spends.is_ok()), 0);
    out.push(footer(&spends, &credit::peers(&places.policy), now.unix_millis()));
    out
}

fn rows(mut entries: Vec<Entry>, read: bool) -> Vec<Vec<String>> {
    entries.sort_by(|a, b| a.order().cmp(&b.order()));
    let mut header: Vec<String> = HEADER.map(str::to_owned).to_vec();
    let mut table: Vec<Vec<String>> = entries.iter().map(|entry| row(entry, read)).collect();
    if table.iter().any(|row| row.len() > HEADER.len()) {
        header.push("then".to_owned());
    }
    table.insert(0, header);
    table
}

fn row(entry: &Entry, read: bool) -> Vec<String> {
    let Some(balance) = &entry.balance else { return vec![entry.name.clone(), "none".to_owned(), UNREAD.to_owned(), UNREAD.to_owned(), UNREAD.to_owned()] };
    let money = |usd: f64| format!("${usd:.2}");
    let (spent, left) = match (read, entry.exhausted()) {
        (false, _) => (UNREAD.to_owned(), UNREAD.to_owned()),
        (true, true) => (money(balance.spent), EXHAUSTED.to_owned()),
        (true, false) => (money(balance.spent), money(balance.left)),
    };
    let mut cells = vec![entry.name.clone(), money(balance.grant.usd), plain::day(&balance.grant.expires), spent, left];
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
