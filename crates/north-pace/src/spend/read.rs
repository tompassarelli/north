//! Gathers a `View` from this host's state: each account's last sampled windows in `usage-gate.json`,
//! its plan size as the caller reads it, and the trips `north fleet tick`
//! wrote to `spend/trips.json`.
use std::path::Path;

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use super::{Account, Burn, Rules, Trip, View, Window};
use crate::foundation::clock::rfc3339_seconds;

/// The current trips, relative to the agents state directory.
pub const TRIPS: &str = "spend/trips.json";

/// The fleet's measured burn per account, relative to the agents state directory.
pub const BURNS: &str = "spend/burns.json";

/// What the caller knows of an account beyond its sampled windows: its week in Max 5x weeks.
pub struct Size {
    pub plan_week: f64,
}

/// The account names the policy declares: those billed by API key and those kept as the operational reserve.
pub struct Declared {
    pub keyed: Vec<String>,
    pub operational: Vec<String>,
}

/// The view at `at` with `working` worker sessions across the fleet, read from the agents state
/// directory `agents`; `size` sizes an account and `signed_in` says whether some host is signed in to it.
pub fn view(agents: &Path, rules: &Rules, working: usize, at: f64, size: impl Fn(&str) -> Size, signed_in: impl Fn(&str) -> bool, declared: Declared) -> View {
    let gate = read(&agents.join("usage-gate.json"));
    let sampled = gate["claude"]["windows"].as_object().into_iter().flatten().filter(|(name, _)| name.starts_with("claude:"));
    let accounts = sampled.map(|(name, windows)| account(rules, name, size(name), windows, at, signed_in(name))).collect();
    View { at, accounts, working, trips: trips(&read(&agents.join(TRIPS)), rules, at), burns: burns(&read(&agents.join(BURNS)), rules, at), keyed: declared.keyed, operational: declared.operational, drain: super::drain::on(agents, at) }
}

pub fn view_from_config(cfg: &DocumentMut, agents: &Path, rules: &Rules, working: usize, at: f64, signed_in: impl Fn(&str) -> bool) -> View {
    let catalog = crate::platform::harness::document(crate::platform::harness::MODEL_CATALOG);
    view(agents, rules, working, at, |name| Size { plan_week: plan_week(cfg, catalog, name) }, signed_in, Declared { keyed: keyed(cfg), operational: operational_reserve(cfg) })
}

/// Trips the last tick confirmed within the window of the burst each measured; a tick that stopped
/// confirming one lets it lapse.
fn trips(listed: &Value, rules: &Rules, at: f64) -> Vec<Trip> {
    let all: Vec<Trip> = serde_json::from_value(listed["trips"].clone()).unwrap_or_default();
    all.into_iter().filter(|trip| at - trip.at <= trip.holds(rules)).collect()
}

/// Burns the last tick measured within the span of the burst it projects from, so a tick that stopped running leaves the governor no rate to project.
fn burns(listed: &Value, rules: &Rules, at: f64) -> Vec<Burn> {
    let all: Vec<Burn> = serde_json::from_value(listed["burns"].clone()).unwrap_or_default();
    all.into_iter().filter(|burn| at - burn.at <= rules.fleet_trip.minutes * 60.0).collect()
}

fn read(path: &Path) -> Value {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

fn account(rules: &Rules, name: &str, size: Size, windows: &Value, at: f64, signed_in: bool) -> Account {
    let found = |wanted: &str| windows.as_array()?.iter().find(|window| window["window"] == wanted);
    let window = |wanted: &str| found(wanted).and_then(|window| Some(Window { used_pct: window["used_pct"].as_f64()?, resets_at: window["resets_at"].as_str().and_then(rfc3339_seconds) }));
    let read_at = found("week").and_then(|week| week["as_of"].as_str()).and_then(rfc3339_seconds);
    let fresh = read_at.is_some_and(|read_at| at - read_at <= rules.reading_max_h * 3600.0);
    Account { name: name.to_owned(), plan_week: size.plan_week, five_hour: window("5h"), week: window("week"), counted: signed_in && fresh }
}

/// The `provider:label` names of the accounts the policy bills by API key (`[accounts.claude.api_keys]`).
pub fn keyed(cfg: &DocumentMut) -> Vec<String> {
    let table = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_keys")).and_then(Item::as_table_like);
    table.into_iter().flat_map(|table| table.iter().map(|(label, _)| format!("claude:{label}"))).collect()
}

/// The `provider:label` names of the accounts the policy declares an operational reserve (`[accounts.claude] reserve`): a plan account by its label (`e`) or an API organization as `api:ORG`.
pub fn operational_reserve(cfg: &DocumentMut) -> Vec<String> {
    let names = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("reserve")).and_then(Item::as_array);
    names.into_iter().flatten().filter_map(|name| name.as_str()).map(|name| format!("claude:{name}")).collect()
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
