//! `usage-gate.json`: each provider's money refusal, per-account refusals and each account's sampled windows,
//! read by the spawn hook without a subprocess.
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Map, Value, json};
use toml_edit::DocumentMut;

use super::ctx::{self, Defaults, Tier};
use super::decide::world::ModelFamily;
use super::{Places, meter, profiles, py, settings};
use crate::foundation::clock::Timestamp;
use crate::router::{ops, policy};

const REFUSAL_TAIL: &str = "Unknown or money-billed usage is never chosen automatically (firn#12). Use another provider's tier from `agents plan`, wait for the window to reset, or report `Blocked: decision for Tom: <question>` for the proxy to put to Tom.";

pub(super) struct Inputs {
    pub cfg: Value,
    pub catalog: Value,
    tiers: Vec<Tier>,
    defaults: Defaults,
}

impl Inputs {
    pub(super) fn load(policy: &Path) -> Result<Self, String> {
        let document: DocumentMut = fs::read_to_string(policy).map_err(|error| error.to_string())?.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
        Ok(Self {
            cfg: policy::json_value(document.as_item()),
            catalog: settings::catalog_json(),
            tiers: tiers(&document),
            defaults: Defaults { min_runs: settings::default_value("min_runs").as_u64().unwrap_or_default() as usize },
        })
    }

    pub(super) fn context(&self, db: &Connection, readings: &[super::store::Row], provider: &str, now: Timestamp) -> Result<Value, String> {
        let rows: Vec<&super::store::Row> = readings.iter().filter(|row| row.text("provider") == Some(provider)).collect();
        ctx::live_ctx(&self.cfg, &self.tiers, &self.catalog, &rows, db, now, &self.defaults)
    }
}

pub(super) fn write(db: &Connection, providers: &[&str], places: &Places, now: Timestamp) -> Result<Value, String> {
    let inputs = Inputs::load(&places.policy)?;
    let (cfg, catalog) = (&inputs.cfg, &inputs.catalog);
    let known: Vec<&str> = catalog["accounts"].as_object().into_iter().flatten().map(|(name, _)| name.as_str()).collect();
    let path = places.state.join("agents/usage-gate.json");
    let mut verdicts: Map<String, Value> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    verdicts.retain(|provider, _| known.contains(&provider.as_str()));
    let readings = super::readings(db, &places.policy)?;
    for provider in providers.iter().filter(|provider| known.contains(*provider)) {
        let context = inputs.context(db, &readings, provider, now)?;
        let local = local_account(provider, cfg);
        let refusal = |account: Option<&str>| refusal(&context, provider, account, local.as_deref());
        let named: Vec<&String> = context["accounts"].as_object().into_iter().flatten().map(|(name, _)| name).filter(|name| name.split(':').next() == Some(provider)).collect();
        let refusals: Map<String, Value> = named.iter().map(|name| ((*name).clone(), refusal(Some(name)))).collect();
        let windows: Map<String, Value> = named.iter().map(|name| sampled(&context, name, provider, now, &readings)).collect::<Result<_, _>>()?;
        let verdict = json!({
            "ts": py::second(now), "epoch": now.unix_seconds_f64(), "stale_min": settings::default_value("usage_stale_min"),
            "refusal": refusal(None), "refusals": refusals, "windows": windows,
        });
        verdicts.insert((*provider).to_owned(), verdict);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let pending = path.with_extension("tmp");
    fs::write(&pending, Value::Object(verdicts.clone()).to_string()).and_then(|()| fs::rename(&pending, &path)).map_err(|error| error.to_string())?;
    Ok(Value::Object(verdicts))
}

fn tiers(document: &DocumentMut) -> Vec<Tier> {
    let Some(table) = document.get("tiers").and_then(toml_edit::Item::as_table_like) else { return Vec::new() };
    table
        .iter()
        .map(|(key, item)| {
            let value = policy::json_value(item);
            let text = |name: &str| value[name].as_str().unwrap_or_default().to_owned();
            Tier { key: key.to_owned(), model: text("model"), effort: text("effort"), start: value["start"].clone() }
        })
        .collect()
}

pub(super) fn account_name(context: &Value, provider: &str, local: Option<&str>) -> String {
    match local {
        Some(label) if context["accounts"].get(provider).is_none() => format!("{provider}:{label}"),
        _ => provider.to_owned(),
    }
}

fn refusal(context: &Value, provider: &str, account: Option<&str>, local: Option<&str>) -> Value {
    let name = account.map_or_else(|| account_name(context, provider, local), str::to_owned);
    let asked = ops::answer(&json!({"op": "money_refusal", "ctx": context, "account": name}));
    match asked {
        Some(Ok(Value::String(why))) if !why.is_empty() => json!(format!("No: a new {name} worker could spend money: {why}. {REFUSAL_TAIL}")),
        Some(Ok(other)) => other,
        _ => Value::Null,
    }
}

fn sampled(context: &Value, name: &str, provider: &str, now: Timestamp, readings: &[super::store::Row]) -> Result<(String, Value), String> {
    let account = &context["accounts"][name];
    let key = if provider == "claude" { format!("claude:{}", account["id"].as_str().unwrap_or("None")) } else { name.to_owned() };
    let mut windows = Vec::new();
    for (kind, window) in account["windows"].as_object().into_iter().flatten() {
        let age = (now.unix_nanos() - py::parse(window["as_of"].as_str().unwrap_or_default())?.unix_nanos()) as f64 / 1e9;
        windows.push(json!({
            "window": kind, "used_pct": 100.0 - window["headroom"].as_f64().unwrap_or_default(), "reset_h": window["reset_in"],
            "age_s": age, "as_of": window["as_of"], "resets_at": window["resets_at"],
        }));
    }
    if provider == "claude" {
        let fable = meter::scoped(ModelFamily::Fable);
        let label = account["id"].as_str().unwrap_or_default();
        if let Some(row) = readings.iter().find(|row| row.text("provider") == Some("claude") && row.text("account") == Some(label) && row.text("kind") == Some("window") && row.text("name") == Some(fable.as_str()))
            && let Some(used) = row.number("used_pct")
        {
            let as_of = row.text("ts").unwrap_or_default();
            let age = (now.unix_nanos() - py::parse(as_of)?.unix_nanos()) as f64 / 1e9;
            let resets_at = row.text("resets_at").filter(|text| !text.is_empty());
            let reset_h = resets_at.and_then(|text| py::parse(text).ok()).map_or(0.0, |at| (at.unix_nanos() - now.unix_nanos()) as f64 / 1e9 / 3600.0).max(0.0);
            windows.push(json!({"window": fable, "used_pct": used, "reset_h": reset_h, "age_s": age, "as_of": as_of, "resets_at": resets_at}));
        }
    }
    Ok((key, Value::Array(windows)))
}

pub(super) fn local_account(provider: &str, cfg: &Value) -> Option<String> {
    if provider != "claude" {
        return None;
    }
    let directory = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|path| !path.is_empty()).map(std::path::PathBuf::from);
    account_in(cfg, &profiles::home(), directory.as_deref(), &crate::platform::host::name())
}

pub(super) fn account_in(cfg: &Value, home: &Path, directory: Option<&Path>, host: &str) -> Option<String> {
    let cache: Value = fs::read_to_string(directory.unwrap_or(home).join(".claude.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    let uuid = cache["oauthAccount"]["accountUuid"].as_str().or_else(|| cache["cachedUsageUtilization"]["accountUuid"].as_str()).filter(|uuid| !uuid.is_empty());
    match uuid {
        Some(uuid) => profiles::label_of(cfg, uuid),
        None => token_account(cfg, directory, host),
    }
}

fn token_account(cfg: &Value, directory: Option<&Path>, host: &str) -> Option<String> {
    let wanted = directory?;
    profiles::profiles(cfg, host).into_iter().find(|profile| profile.token_file && profile.path.as_deref() == Some(wanted)).map(|profile| profile.label)
}
