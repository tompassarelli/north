//! Gathers a `View` from this host's state: each account's last sampled windows in `usage-gate.json`,
//! its plan size as the caller reads it, and the trips `north fleet tick`
//! wrote to `spend/trips.json`.
use std::path::Path;

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use super::{Account, Rules, Trip, View, Window};
use crate::foundation::clock::rfc3339_seconds;

/// The current trips, relative to the agents state directory.
pub const TRIPS: &str = "spend/trips.json";

/// The view at `at` with `working` worker sessions across the fleet, read from the agents state
/// directory `agents`; `plan_week` sizes an account's week in Max 5x weeks and `signed_in` says
/// whether some host is signed in to it.
pub fn view(agents: &Path, rules: &Rules, working: usize, at: f64, plan_week: impl Fn(&str) -> f64, signed_in: impl Fn(&str) -> bool, keyed: Vec<String>) -> View {
    let gate = read(&agents.join("usage-gate.json"));
    let sampled = gate["claude"]["windows"].as_object().into_iter().flatten().filter(|(name, _)| name.starts_with("claude:"));
    let accounts = sampled.map(|(name, windows)| account(rules, name, plan_week(name), windows, at, signed_in(name))).collect();
    View { at, accounts, working, trips: trips(&read(&agents.join(TRIPS)), rules, at), keyed }
}

/// Trips the last tick confirmed within the longer burst's span; a tick that stopped confirming one
/// lets it lapse.
fn trips(listed: &Value, rules: &Rules, at: f64) -> Vec<Trip> {
    let all: Vec<Trip> = serde_json::from_value(listed["trips"].clone()).unwrap_or_default();
    let span = rules.fleet_trip.minutes.max(rules.session_trip.minutes) * 60.0;
    all.into_iter().filter(|trip| at - trip.at <= span).collect()
}

fn read(path: &Path) -> Value {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

fn account(rules: &Rules, name: &str, plan_week: f64, windows: &Value, at: f64, signed_in: bool) -> Account {
    let found = |wanted: &str| windows.as_array()?.iter().find(|window| window["window"] == wanted);
    let window = |wanted: &str| found(wanted).and_then(|window| Some(Window { used_pct: window["used_pct"].as_f64()?, resets_at: window["resets_at"].as_str().and_then(rfc3339_seconds) }));
    let read_at = found("week").and_then(|week| week["as_of"].as_str()).and_then(rfc3339_seconds);
    let fresh = read_at.is_some_and(|read_at| at - read_at <= rules.reading_max_h * 3600.0);
    Account { name: name.to_owned(), plan_week, five_hour: window("5h"), week: window("week"), counted: signed_in && fresh }
}

/// The `provider:label` names of the accounts the policy bills by API key (`[accounts.claude.api_keys]`).
pub fn keyed(cfg: &DocumentMut) -> Vec<String> {
    let table = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_keys")).and_then(Item::as_table_like);
    table.into_iter().flat_map(|table| table.iter().map(|(label, _)| format!("claude:{label}"))).collect()
}

/// The account's week in Max 5x weeks: the weekly multiplier of its `[accounts.PROVIDER] plans`
/// entry in the model catalog `catalog` over that of `max-5x`, or 1 when either is missing.
pub fn plan_week(cfg: &DocumentMut, catalog: &DocumentMut, account: &str) -> f64 {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let plans = catalog.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|catalog| catalog.get("plans"));
    let weekly = |plan: &str| plans?.get(plan)?.get("weekly_multiplier").and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))).filter(|times| *times > 0.0);
    let named = cfg.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|policy| policy.get("plans")).and_then(|plans| plans.get(label)).and_then(Item::as_str);
    named.and_then(weekly).zip(weekly("max-5x")).map_or(1.0, |(plan, unit)| plan / unit)
}
