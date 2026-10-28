use std::collections::{BTreeMap, BTreeSet};

use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::foundation::clock::Timestamp;
use crate::router::ctx::Knobs;

use super::super::tiers::{Tier, provider};
use super::config::Config;
use super::ledger::{Ledger, instant, python_iso, shifted};

const MODES: [&str; 4] = ["standard", "fast", "ultrafast", "overflow"];

#[derive(Deserialize)]
pub(super) struct Reading {
    pub ts: String,
    pub provider: String,
    pub account: String,
    pub kind: String,
    pub name: String,
    pub used_pct: Option<f64>,
    pub window_min: Option<f64>,
    pub resets_at: Option<String>,
    pub amount: Option<f64>,
    pub source: Option<String>,
}

fn window_key(reading: &Reading) -> Option<String> {
    if reading.window_min == Some(300.0) {
        return Some("5h".to_owned());
    }
    let weekly = reading.name == "week"
        || reading.name == "weekly_all"
        || (reading.provider == "gemini" && reading.window_min == Some(10080.0));
    if weekly {
        Some("week".to_owned())
    } else if reading.name.contains('/') {
        None
    } else {
        Some(reading.name.clone())
    }
}

fn beta(ledger: &Ledger, reading: &Reading, now: Timestamp) -> Option<f64> {
    let db = ledger.0.as_ref()?;
    let resets = instant(reading.resets_at.as_deref()?)?;
    let window = reading.window_min?;
    let since = shifted(resets, -window * 60.0).max(shifted(now, -3.0 * 3600.0));
    let old: Option<(String, f64)> = db
        .query_row(
            "SELECT ts, used_pct FROM usage WHERE provider = ?1 AND account = ?2 AND kind = 'window' AND name = ?3 \
             AND resets_at = ?4 AND ts >= ?5 AND ts < ?6 AND used_pct IS NOT NULL ORDER BY ts LIMIT 1",
            rusqlite::params![reading.provider, reading.account, reading.name, reading.resets_at, python_iso(since), reading.ts],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    let hours = old.as_ref().map_or(0.0, |(at, _)| match (instant(&reading.ts), instant(at)) {
        (Some(read), Some(earlier)) => (read.unix_nanos() - earlier.unix_nanos()) as f64 / 3.6e12,
        _ => 0.0,
    });
    let (_, earlier_pct) = old?;
    let used = reading.used_pct?;
    (hours >= 1.0 / 6.0).then(|| (used - earlier_pct).max(0.0) / hours)
}

struct Account {
    id: String,
    windows: Map<String, Value>,
    flags: BTreeMap<&'static str, bool>,
    age_min: Option<f64>,
    plan: Option<String>,
}

fn accounts_from(rows: &[Reading], ledger: &Ledger, now: Timestamp) -> BTreeMap<String, Account> {
    let mut ids: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for row in rows {
        ids.entry(&row.provider).or_default().insert(&row.account);
    }
    let mut newest_first: Vec<&Reading> = rows.iter().collect();
    newest_first.sort_by(|left, right| right.ts.cmp(&left.ts));
    let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
    for row in newest_first {
        let key = if ids[row.provider.as_str()].len() == 1 { row.provider.clone() } else { format!("{}:{}", row.provider, row.account) };
        let account = accounts.entry(key).or_insert_with(|| Account {
            id: row.account.clone(),
            windows: Map::new(),
            flags: BTreeMap::new(),
            age_min: None,
            plan: None,
        });
        if row.kind != "grant" {
            let age = instant(&row.ts).map_or(0.0, |read| (now.unix_nanos() - read.unix_nanos()) as f64 / 6e10);
            account.age_min = Some(account.age_min.map_or(age, |known| known.max(age)));
        }
        if row.kind == "window" {
            let (Some(used), Some(resets)) = (row.used_pct, row.resets_at.as_deref().filter(|text| !text.is_empty())) else {
                continue;
            };
            let Some(name) = window_key(row) else {
                continue;
            };
            let headroom = 100.0 - used;
            let lower = account.windows.get(&name).and_then(|known| known["headroom"].as_f64()).is_none_or(|known| row.provider == "gemini" && headroom < known);
            if lower {
                let reset_in = instant(resets).map_or(0.0, |at| ((at.unix_nanos() - now.unix_nanos()) as f64 / 3.6e12).max(0.0));
                account.windows.insert(
                    name,
                    json!({"headroom": headroom, "beta": beta(ledger, row, now), "reset_in": reset_in, "resets_at": resets,
                           "window_h": row.window_min.unwrap_or_default() / 60.0, "as_of": row.ts}),
                );
            }
        } else if row.kind == "billing" && matches!(row.name.as_str(), "hasCredits" | "extra_usage" | "spend") {
            let flag = ["hasCredits", "extra_usage", "spend"].into_iter().find(|name| *name == row.name).unwrap_or_default();
            account.flags.insert(flag, row.amount.is_some_and(|amount| amount != 0.0));
        } else if row.kind == "billing" && row.name == "plan" && account.plan.is_none() {
            account.plan = row.source.clone();
        }
    }
    accounts
}

fn prices(costs: &Value) -> BTreeMap<String, f64> {
    if let Some(services) = costs.get("service").and_then(Value::as_array) {
        return services
            .iter()
            .filter(|service| service["kind"] == "ai-plan")
            .filter_map(|service| Some((service["id"].as_str()?.to_owned(), service["usd_per_week"].as_f64()?)))
            .collect();
    }
    costs.as_object().into_iter().flatten().filter_map(|(id, price)| Some((id.clone(), price.as_f64()?))).collect()
}

fn account_json(name: &str, account: Account, config: &Config, prices: &BTreeMap<String, f64>, knobs: &Knobs) -> Value {
    let provider = provider(name);
    let settings = config.account_settings(provider);
    let cost_id = if provider == "claude" && !account.id.is_empty() { format!("claude-{}", account.id) } else { provider.to_owned() };
    let required = settings.get("auto_route_requires").and_then(|required| required.get(&account.id)).and_then(Value::as_str);
    let auto_route = required.is_none_or(|plan| account.plan.as_deref() == Some(plan));
    let flag = |name: &str| account.flags.get(name).copied().unwrap_or(false);
    json!({
        "id": account.id, "windows": account.windows, "grants": [], "hasCredits": flag("hasCredits"),
        "extra_usage": flag("extra_usage"), "spend": flag("spend"), "age_min": account.age_min,
        "auto_route": auto_route, "provider": provider, "usd_per_week": prices.get(&cost_id).copied().unwrap_or(0.0),
        "target_left": settings.get("target_left").cloned().unwrap_or_else(|| json!(knobs.pace_target_left_claude)),
        "band": settings.get("band").cloned().unwrap_or_else(|| json!(knobs.pace_band)),
        "thursday": settings.get("thursday").cloned().unwrap_or(json!(false)),
        "plan": account.plan,
    })
}

pub(super) fn context(config: &Config, catalog: &Value, rows: &[Reading], ledger: &Ledger, now: Timestamp, owned: &[Tier], costs: &Value) -> Value {
    let knobs = Knobs::default();
    let billing: Map<String, Value> = catalog["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, account)| {
            let kept: Map<String, Value> = account.as_object().into_iter().flatten().filter(|(key, _)| MODES.contains(&key.as_str())).map(|(key, value)| (key.clone(), value.clone())).collect();
            (name.clone(), Value::Object(kept))
        })
        .collect();
    let projects: Map<String, Value> = config
        .projects()
        .map(|(name, project)| {
            let mut entry = project.as_object().cloned().unwrap_or_default();
            entry.insert("u".to_owned(), json!(1.0));
            entry.insert("unmeasured".to_owned(), json!(entry.contains_key("target")));
            (name.to_owned(), Value::Object(entry))
        })
        .collect();
    let (success, runs) = ledger.measured(owned, knobs.min_runs as usize);
    let due = ledger.successors(now, owned);
    let tier_values: Vec<Value> = owned
        .iter()
        .enumerate()
        .map(|(rank, tier)| {
            let mut entry = json!({"key": tier.key, "model": tier.model, "effort": tier.effort, "rank": rank, "start": tier.start, "runs": runs[&tier.key]});
            if let Some(successor) = due.get(&tier.key) {
                entry["successor"] = successor.clone();
            }
            entry
        })
        .collect();
    let prices = prices(costs);
    let accounts: Map<String, Value> =
        accounts_from(rows, ledger, now).into_iter().map(|(name, account)| { let value = account_json(&name, account, config, &prices, &knobs); (name, value) }).collect();
    json!({
        "now": now.iso_micros_utc_offset(), "tiers": tier_values, "billing": billing, "lease": false, "success": success,
        "accounts": accounts, "projects": projects, "boxes": [], "pace": config.pace(),
        "services": costs.get("service").cloned().unwrap_or_else(|| json!([])),
    })
}
