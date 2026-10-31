//! The router's ctx from orchestration.toml, the catalog's billing modes and the usage snapshot.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::types::Value as Cell;
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};

use super::store::Row;
use super::{burn, py};
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
    pub target_left_claude: Value,
    pub pace_band: Value,
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
    let due = successors(db, now, tiers)?;
    let listed: Vec<Value> = tiers
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
    let mut accounts = accounts_from(rows, db, now)?;
    configure_accounts(&mut accounts, cfg, defaults);
    Ok(json!({
        "now": py::second(now), "tiers": listed, "billing": billing, "lease": false, "success": success,
        "accounts": accounts, "projects": projects, "boxes": [], "pace": cfg.get("pace").cloned().unwrap_or_else(|| json!({})),
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

fn accounts_from(rows: &[&Row], db: &Connection, now: Timestamp) -> Result<Map<String, Value>, String> {
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
                    "headroom": headroom, "beta": burn::beta(db, row, now)?, "reset_in": reset_in.max(0.0), "resets_at": resets_at,
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
        roll_over_expired(&mut accounts[&key], py::parse(read_at)?)?;
    }
    Ok(accounts)
}

fn key_of(provider: &str, accounts: usize, row: &Row) -> String {
    if accounts == 1 { provider.to_owned() } else { format!("{provider}:{}", row.text("account").unwrap_or("None")) }
}

/// A window whose reset the account's newest reading postdates has rolled over, and a provider that reports no
/// reading for a window it has not opened leaves its last row behind: the window reads 0% used as of that reading.
fn roll_over_expired(account: &mut Value, read_at: Timestamp) -> Result<(), String> {
    let stamp = py::second(read_at);
    for window in account["windows"].as_object_mut().into_iter().flat_map(Map::values_mut) {
        let reset = py::parse(window["resets_at"].as_str().unwrap_or_default())?;
        if reset.unix_nanos() <= read_at.unix_nanos() && window["as_of"].as_str().is_some_and(|as_of| py::parse(as_of).is_ok_and(|as_of| as_of.unix_nanos() < reset.unix_nanos())) {
            window["headroom"] = json!(100.0);
            window["beta"] = json!(0.0);
            window["as_of"] = json!(stamp);
        }
    }
    Ok(())
}

fn configure_accounts(accounts: &mut Map<String, Value>, cfg: &Value, defaults: &Defaults) {
    for (name, account) in accounts.iter_mut() {
        let provider = name.split(':').next().unwrap_or_default().to_owned();
        let settings = &cfg["accounts"][&provider];
        let label = account["id"].as_str().map(str::to_owned);
        let required = label.as_deref().and_then(|label| settings["auto_route_requires"].get(label)).cloned();
        let auto_route = required.as_ref().is_none_or(|plan| account.get("plan") == Some(plan));
        account["auto_route"] = json!(auto_route);
        account["provider"] = json!(provider);
        account["usd_per_week"] = json!(0.0);
        account["target_left"] = settings.get("target_left").cloned().unwrap_or_else(|| defaults.target_left_claude.clone());
        account["band"] = settings.get("band").cloned().unwrap_or_else(|| defaults.pace_band.clone());
        account["thursday"] = settings.get("thursday").cloned().unwrap_or(json!(false));
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

fn lineage(model: &str) -> (Vec<String>, Vec<i64>) {
    let name = model.split_once(':').map_or(model, |(_, rest)| rest);
    let parts: Vec<&str> = name.split(['-', '.']).collect();
    let words = parts.iter().filter(|part| !part.chars().all(|c| c.is_ascii_digit()) || part.is_empty()).map(|part| (*part).to_owned()).collect();
    let numbers = parts.iter().filter(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())).filter_map(|part| part.parse().ok()).collect();
    (words, numbers)
}

fn successors(db: &Connection, now: Timestamp, tiers: &[Tier]) -> Result<BTreeMap<String, Value>, String> {
    let mut latest: BTreeMap<String, (String, Option<f64>)> = BTreeMap::new();
    for row in query(db, "SELECT model, at, p FROM model_events WHERE kind IN ('expected', 'market') AND at IS NOT NULL ORDER BY ts") {
        let p = match row[2] {
            Cell::Real(p) => Some(p),
            Cell::Integer(p) => Some(p as f64),
            _ => None,
        };
        if let (Some(model), Some(at)) = (text(&row[0]), text(&row[1])) {
            latest.insert(model, (at, p));
        }
    }
    let mut out = BTreeMap::new();
    for tier in tiers {
        let (family, version) = lineage(&tier.model);
        let mut due: Vec<(f64, f64, String)> = Vec::new();
        for (model, (at, p)) in &latest {
            let when = py::parse(at)?;
            let (candidate, newer) = lineage(model);
            if when >= now && candidate == family && newer > version {
                due.push(((when.unix_nanos() - now.unix_nanos()) as f64 / 1e9 / 86400.0, p.unwrap_or(1.0), model.clone()));
            }
        }
        due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        if let Some((eta, p, model)) = due.into_iter().next() {
            out.insert(tier.key.clone(), json!({"eta_days": eta, "p": p, "model": model}));
        }
    }
    Ok(out)
}
