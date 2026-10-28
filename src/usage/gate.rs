//! `usage-gate.json`: each provider's money refusal, per-account refusals, window projections and pace reading,
//! read by the spawn hook without a subprocess.
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use super::ctx::{self, Defaults, Tier};
use super::{Places, py, settings};
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
            defaults: Defaults {
                min_runs: settings::default_value("min_runs").as_u64().unwrap_or_default() as usize,
                target_left_claude: settings::default_value("pace_target_left_claude"),
                pace_band: settings::default_value("pace_band"),
            },
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
        let projections: Map<String, Value> = named.iter().map(|name| projection(&context, name, provider, now).map(|found| (found.0, found.1))).collect::<Result<_, _>>()?;
        let mut verdict = json!({
            "ts": py::second(now), "epoch": now.unix_seconds_f64(), "stale_min": settings::default_value("usage_stale_min"), "pace_control": true,
            "controls": verdicts.get(*provider).and_then(|previous| previous.get("controls")).cloned().unwrap_or_else(|| json!({})),
            "refusal": refusal(None), "refusals": refusals, "projections": projections,
        });
        if let Some(Ok(pace)) = ops::answer(&json!({"op": "gate", "ctx": context, "provider": provider})) {
            for (key, value) in pace.as_object().into_iter().flatten() {
                verdict[key] = value.clone();
            }
        }
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

fn refusal(context: &Value, provider: &str, account: Option<&str>, local: Option<&str>) -> Value {
    let name = match account {
        Some(name) => name.to_owned(),
        None => match local {
            Some(label) if context["accounts"].get(provider).is_none() => format!("{provider}:{label}"),
            _ => provider.to_owned(),
        },
    };
    let asked = ops::answer(&json!({"op": "money_refusal", "ctx": context, "account": name}));
    match asked {
        Some(Ok(Value::String(why))) if !why.is_empty() => json!(format!("No: a new {name} worker could spend money: {why}. {REFUSAL_TAIL}")),
        Some(Ok(other)) => other,
        _ => Value::Null,
    }
}

fn projection(context: &Value, name: &str, provider: &str, now: Timestamp) -> Result<(String, Value), String> {
    let account = &context["accounts"][name];
    let key = if provider == "claude" { format!("claude:{}", account["id"].as_str().unwrap_or("None")) } else { name.to_owned() };
    let ceiling = quota_ceiling(&account["target_left"]);
    let mut windows = Vec::new();
    for (kind, window) in account["windows"].as_object().into_iter().flatten() {
        let (headroom, reset_in) = (window["headroom"].as_f64().unwrap_or_default(), window["reset_in"].as_f64().unwrap_or_default());
        let burn = window["beta"].as_f64();
        let age = (now.unix_nanos() - py::parse(window["as_of"].as_str().unwrap_or_default())?.unix_nanos()) as f64 / 1e9;
        windows.push(json!({
            "window": kind, "used_pct": 100.0 - headroom, "quota_ceiling": ceiling, "burn_pct_h": window["beta"], "reset_h": window["reset_in"],
            "age_s": age, "as_of": window["as_of"], "resets_at": window["resets_at"],
            "projected_pct": burn.map(|burn| 100.0 - headroom + burn * reset_in),
        }));
    }
    Ok((key, Value::Array(windows)))
}

fn quota_ceiling(target_left: &Value) -> Value {
    let plan_stop = settings::default_value("plan_stop");
    let left = match (target_left.as_i64(), target_left.as_f64()) {
        (Some(whole), _) => json!(100 - whole),
        (None, Some(fraction)) => json!(100.0 - fraction),
        _ => return plan_stop,
    };
    if plan_stop.as_f64() <= left.as_f64() { plan_stop } else { left }
}

fn local_account(provider: &str, cfg: &Value) -> Option<String> {
    if provider != "claude" {
        return None;
    }
    let directory = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|path| !path.is_empty()).map(std::path::PathBuf::from);
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let cache: Value = fs::read_to_string(directory.as_deref().unwrap_or(&home).join(".claude.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    let uuid = cache["oauthAccount"]["accountUuid"].as_str().or_else(|| cache["cachedUsageUtilization"]["accountUuid"].as_str()).filter(|uuid| !uuid.is_empty());
    match uuid {
        Some(uuid) => {
            let digest = Sha256::digest(uuid.as_bytes());
            let hash: String = digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
            cfg["accounts"]["claude"]["labels"][&hash].as_str().map(str::to_owned)
        }
        None => token_account(cfg, directory.as_deref()),
    }
}

fn token_account(cfg: &Value, directory: Option<&Path>) -> Option<String> {
    let host = crate::platform::host::name();
    let wanted = directory?.to_string_lossy().into_owned();
    cfg["accounts"]["claude"]["profiles"].as_object()?.iter().find_map(|(label, hosts)| {
        let profile = hosts.get(&host)?;
        let path = profile.get("path").and_then(Value::as_str).map(expand);
        (profile.get("token_file").is_some() && path.as_deref() == Some(wanted.as_str())).then(|| label.clone())
    })
}

fn expand(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
        None => path.to_owned(),
    }
}
