//! The router's ctx from orchestration.toml, the catalog's billing modes and the usage snapshot.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::types::Value as Cell;
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};

use super::store::Row;
use super::py;
use crate::foundation::clock::Timestamp;

const MODES: [&str; 4] = ["standard", "fast", "ultrafast", "overflow"];
const RUN_TIERS: [(&str, &str); 10] = [
    ("claude:haiku", "haiku"),
    ("claude:sonnet-high", "sonnet"),
    ("claude:opus-medium", "medium"),
    ("claude:opus-high", "high"),
    ("claude:opus-xhigh", "xhigh"),
    ("codex:luna-low", "gpt-6-luna low"),
    ("codex:sol-medium", "gpt-6.1-sol medium"),
    ("codex:sol-high", "gpt-6.1-sol high"),
    ("codex:sol-xhigh", "gpt-6.1-sol xhigh"),
    ("codex:astra", "gpt-6-astra xhigh"),
];

pub(super) struct Tier {
    pub key: String,
    pub model: String,
    pub effort: String,
    pub start: Value,
}

pub(super) struct Defaults {
    pub min_runs: usize,
}

pub(super) fn live_ctx(cfg: &Value, tiers: &[Tier], catalog: &Value, rows: &[&Row], db: &Connection, now: Timestamp, defaults: &Defaults) -> Result<Value, String> {
    let billing: Map<String, Value> = catalog["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, account)| (name.clone(), Value::Object(MODES.iter().filter_map(|mode| Some(((*mode).to_owned(), account.get(*mode)?.clone()))).collect())))
        .collect();
    let projects: Map<String, Value> = cfg["projects"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, project)| {
            let mut project = project.as_object().cloned().unwrap_or_default();
            let unmeasured = project.contains_key("target");
            project.insert("u".into(), json!(1.0));
            project.insert("unmeasured".into(), json!(unmeasured));
            (name.clone(), Value::Object(project))
        })
        .collect();
    let (success, runs) = measured(db, tiers, defaults.min_runs);
    let listed: Vec<Value> = tiers
        .iter()
        .enumerate()
        .map(|(rank, tier)| json!({"key": tier.key, "model": tier.model, "effort": tier.effort, "rank": rank, "start": tier.start, "runs": runs[&tier.key]}))
        .collect();
    let mut accounts = accounts_from(rows, now)?;
    configure_accounts(&mut accounts, cfg);
    Ok(json!({
        "now": py::second(now), "tiers": listed, "billing": billing, "lease": false, "success": success,
        "accounts": accounts, "projects": projects, "boxes": [],
        "services": [],
    }))
}

fn window_key(row: &Row) -> Option<String> {
    let (name, provider) = (row.text("name").unwrap_or_default(), row.text("provider"));
    let minutes = row.number("window_min");
    if minutes == Some(300.0) {
        return Some("5h".into());
    }
    if matches!(name, "week" | "weekly_all") || (provider == Some("gemini") && minutes == Some(10080.0)) {
        return Some("week".into());
    }
    (!name.contains('/')).then(|| name.to_owned())
}

fn accounts_from(rows: &[&Row], now: Timestamp) -> Result<Map<String, Value>, String> {
    let mut ids: BTreeMap<&str, BTreeSet<Option<&str>>> = BTreeMap::new();
    for row in rows {
        ids.entry(row.text("provider").unwrap_or_default()).or_default().insert(row.text("account"));
    }
    let mut latest_first = rows.to_vec();
    latest_first.sort_by(|a, b| b.text("ts").cmp(&a.text("ts")));
    let mut accounts = Map::new();
    let mut newest: BTreeMap<String, &str> = BTreeMap::new();
    for row in latest_first {
        let provider = row.text("provider").unwrap_or_default();
        let key = key_of(provider, ids[provider].len(), row);
        let account = accounts.entry(key).or_insert_with(|| {
            json!({"id": row.text("account"), "windows": {}, "grants": [], "hasCredits": false, "extra_usage": false, "spend": false, "age_min": null, "resets": {}})
        });
        let kind = row.text("kind");
        if kind != Some("grant") {
            let age = py::minutes_precise(py::parse(row.text("ts").unwrap_or_default())?, now);
            let previous = account["age_min"].as_f64();
            account["age_min"] = json!(previous.map_or(age, |previous| previous.min(age)));
        }
        let window = row.number("used_pct").zip(row.text("resets_at").filter(|text| !text.is_empty()));
        if let (Some("window"), Some((used, resets_at))) = (kind, window) {
            let Some(name) = window_key(row) else { continue };
            newest.entry(key_of(provider, ids[provider].len(), row)).or_insert_with(|| row.text("ts").unwrap_or_default());
            let headroom = 100.0 - used;
            let better = provider == "gemini" && account["windows"][&name]["headroom"].as_f64().is_some_and(|held| headroom < held);
            if account["windows"].get(&name).is_none() || better {
                let reset_in = (py::parse(resets_at)?.unix_nanos() - now.unix_nanos()) as f64 / 1e9 / 3600.0;
                account["windows"][&name] = json!({
                    "headroom": headroom, "reset_in": reset_in.max(0.0), "resets_at": resets_at,
                    "window_h": row.number("window_min").unwrap_or_default() / 60.0, "as_of": row.text("ts"),
                });
                account["resets"][&name] = json!(resets_at);
            }
        } else if kind == Some("billing") && matches!(row.text("name"), Some("hasCredits" | "extra_usage" | "spend")) {
            account[row.text("name").unwrap_or_default()] = json!(row.number("amount").is_some_and(|amount| amount != 0.0));
        } else if kind == Some("billing") && row.text("name") == Some("plan") && account.get("plan").is_none() {
            account["plan"] = json!(row.text("source"));
        }
    }
    for (key, read_at) in newest {
        drop_expired(&mut accounts[&key], py::parse(read_at)?)?;
    }
    Ok(accounts)
}

fn key_of(provider: &str, accounts: usize, row: &Row) -> String {
    if accounts == 1 { provider.to_owned() } else { format!("{provider}:{}", row.text("account").unwrap_or("None")) }
}

/// A window whose reset the account's newest reading postdates, with no reading of its own since, is unread: the
/// provider stopped reporting it (a setup token's response omits the 5h headers once the week is rejected, and
/// omitted them on accounts a and b from 2026-10-31 while they worked), so it leaves the account's windows rather
/// than reading 0% used.
fn drop_expired(account: &mut Value, read_at: Timestamp) -> Result<(), String> {
    let mut expired = Vec::new();
    for (name, window) in account["windows"].as_object().into_iter().flatten() {
        let reset = py::parse(window["resets_at"].as_str().unwrap_or_default())?;
        if reset.unix_nanos() <= read_at.unix_nanos() && window["as_of"].as_str().is_some_and(|as_of| py::parse(as_of).is_ok_and(|as_of| as_of.unix_nanos() < reset.unix_nanos())) {
            expired.push(name.clone());
        }
    }
    for name in expired {
        for kind in ["windows", "resets"] {
            if let Some(map) = account[kind].as_object_mut() {
                map.remove(&name);
            }
        }
    }
    Ok(())
}

fn configure_accounts(accounts: &mut Map<String, Value>, cfg: &Value) {
    for (name, account) in accounts.iter_mut() {
        let provider = name.split(':').next().unwrap_or_default().to_owned();
        let settings = &cfg["accounts"][&provider];
        let label = account["id"].as_str().map(str::to_owned);
        let required = label.as_deref().and_then(|label| settings["auto_route_requires"].get(label)).cloned();
        let auto_route = required.as_ref().is_none_or(|plan| account.get("plan") == Some(plan));
        account["auto_route"] = json!(auto_route);
        account["provider"] = json!(provider);
        account["usd_per_week"] = json!(0.0);
    }
}

fn query(db: &Connection, sql: &str) -> Vec<Vec<Cell>> {
    let Ok(mut statement) = db.prepare(sql) else { return Vec::new() };
    let width = statement.column_count();
    let Ok(rows) = statement.query_map(params![], |row| (0..width).map(|index| row.get::<_, Cell>(index)).collect::<Result<Vec<_>, _>>()) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn text(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Text(text) => Some(text.clone()),
        _ => None,
    }
}

fn run_tier(tier: &Tier) -> String {
    RUN_TIERS.iter().find(|(key, _)| *key == tier.key).map_or_else(|| format!("{} {}", tier.model, tier.effort), |(_, name)| (*name).to_owned())
}

fn measured(db: &Connection, tiers: &[Tier], min_runs: usize) -> (Map<String, Value>, BTreeMap<String, usize>) {
    let keys: BTreeMap<String, &str> = tiers.iter().map(|tier| (run_tier(tier), tier.key.as_str())).collect();
    let escalated: BTreeSet<String> = query(db, "SELECT follows FROM runs WHERE role = 'worker' AND follows IS NOT NULL").iter().filter_map(|row| text(&row[0])).collect();
    let mut runs: BTreeMap<String, usize> = tiers.iter().map(|tier| (tier.key.clone(), 0)).collect();
    let mut cells: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    let sql = "SELECT agent, tier, category, outcome, expects_landing, landed, item FROM runs WHERE role = 'worker' AND follows IS NULL AND outcome != 'split'";
    for row in query(db, sql) {
        let Some(key) = text(&row[1]).and_then(|tier| keys.get(&tier).copied()) else { continue };
        let outcome = text(&row[3]);
        let unjudged = matches!(outcome.as_deref(), Some("running" | "idle")) || (outcome.as_deref() == Some("unclear") && !truthy(&row[6]));
        if unjudged {
            continue;
        }
        *runs.entry(key.to_owned()).or_default() += 1;
        let followed = text(&row[0]).is_some_and(|agent| escalated.contains(&agent));
        let blocked = truthy(&row[4]) && matches!(row[5], Cell::Integer(0));
        let category = text(&row[2]).unwrap_or_else(|| "None".to_owned());
        cells.entry(format!("{key}|{category}")).or_default().push(outcome.as_deref() == Some("done") && !followed && !blocked);
    }
    let success = cells.into_iter().filter(|(_, oks)| oks.len() >= min_runs).map(|(cell, oks)| (cell, json!(oks.iter().filter(|ok| **ok).count() * 100 / oks.len()))).collect();
    (success, runs)
}

fn truthy(cell: &Cell) -> bool {
    match cell {
        Cell::Null => false,
        Cell::Integer(number) => *number != 0,
        Cell::Real(number) => *number != 0.0,
        Cell::Text(text) => !text.is_empty(),
        Cell::Blob(bytes) => !bytes.is_empty(),
    }
}
