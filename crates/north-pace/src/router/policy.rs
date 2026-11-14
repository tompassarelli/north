use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item};

/// Runs each arm needs in a category before dispatch routes it by the ledger's pick.
pub const ARM_RUNS: usize = 15;
/// Closed boxes an arm needs in a category before it takes part in the share draws; a thinner arm holds the floor
/// share. Source: `threads backtest` arm-shares walk-forward over the 14 days to 2026-11-14 (1,895 boxes, 32
/// experiments, the 30 days before as prior), USD per kept box against equal shares 0.343: 0.338 at 1, 0.336 at
/// 2-5, 0.335 at 6-8, 0.343 at 10 and 15; the week before (571 boxes) is flat at every value. 6 is the smallest
/// best value on one window; a second window of over 1,000 boxes that separates the values would settle it.
pub const ARM_EVIDENCE: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteValue {
    Expiring,
    ClaudeOverCodex,
    CheapestTier,
    OffLaptop,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouterPolicy {
    pub source: String,
    pub order: Vec<RouteValue>,
    pub five_hour_target: f64,
    pub five_hour_ceiling: f64,
    pub hosts: Vec<String>,
    pub cloud_credit: String,
    pub codex_cloud: bool,
    pub private_repos: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Split,
    ClaudeOnly,
    CodexOnly,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountPolicy {
    #[serde(default)]
    pub auto_route_requires: BTreeMap<String, String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, BTreeMap<String, Profile>>,
    #[serde(default)]
    pub plans: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Profile {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProjectPolicy {
    pub target: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tier {
    #[serde(default)]
    pub key: String,
    pub model: String,
    pub effort: String,
    pub start: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostPolicy {
    pub resident_cap: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogAccount {
    pub standard: String,
    pub fast: Option<String>,
    pub overflow: Option<String>,
    pub auto_purchase: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Defaults {
    pub codex_route: BTreeMap<String, String>,
    pub codex_tier: BTreeMap<String, Vec<String>>,
    pub codex_eta_max_min: u64,
    pub codex_files_max: u64,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub router: RouterPolicy,
    pub mode: Mode,
    pub accounts: BTreeMap<String, AccountPolicy>,
    pub projects: BTreeMap<String, ProjectPolicy>,
    pub tiers: Vec<Tier>,
    pub on_request: Vec<Tier>,
    pub hosts: BTreeMap<String, HostPolicy>,
    pub catalog: BTreeMap<String, CatalogAccount>,
    pub defaults: Defaults,
}

pub fn json_value(item: &Item) -> Value {
    if let Some(table) = item.as_table_like() {
        return Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_owned(), json_value(v)))
                .collect(),
        );
    }
    if let Some(array) = item.as_array_of_tables() {
        return Value::Array(
            array
                .iter()
                .map(|t| json_value(&Item::Table(t.clone())))
                .collect(),
        );
    }
    match item.as_value() {
        Some(toml_edit::Value::String(v)) => v.value().clone().into(),
        Some(toml_edit::Value::Integer(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Float(v)) => {
            serde_json::Number::from_f64(*v.value()).map_or(Value::Null, Value::Number)
        }
        Some(toml_edit::Value::Boolean(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Datetime(v)) => v.value().to_string().into(),
        Some(toml_edit::Value::Array(a)) => Value::Array(
            a.iter()
                .map(|v| json_value(&Item::Value(v.clone())))
                .collect(),
        ),
        _ => Value::Null,
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, key: &str) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("{key}: {error}"))
}

fn tiers(document: &DocumentMut, key: &str) -> Result<Vec<Tier>, String> {
    let Some(table) = document.get(key).and_then(Item::as_table_like) else {
        return Ok(Vec::new());
    };
    table
        .iter()
        .map(|(name, item)| {
            let mut tier: Tier = decode(json_value(item), &format!("{key}.{name}"))?;
            tier.key = name.to_owned();
            Ok(tier)
        })
        .collect()
}

fn field<T: serde::de::DeserializeOwned>(json: &Value, name: &str) -> Result<T, String> {
    decode(json[name]["value"].clone(), name)
}

pub fn validate_defaults(defaults: &str) -> Result<(), String> {
    parse_defaults(defaults).map(drop)
}

pub fn parse_defaults(defaults: &str) -> Result<Defaults, String> {
    let defaults_doc = defaults
        .parse::<DocumentMut>()
        .map_err(|e| format!("defaults: {e}"))?;
    let defaults_json = json_value(defaults_doc.as_item());
    let mut codex_route = BTreeMap::new();
    let mut codex_tier = BTreeMap::new();
    for (name, entry) in defaults_json
        .as_object()
        .ok_or("defaults: expected table")?
    {
        if let Some(category) = name.strip_prefix("codex_route_") {
            let route: String = decode(entry["value"].clone(), name)?;
            if !matches!(route.as_str(), "claude" | "codex") {
                return Err(format!("{name}: unknown value {route}"));
            }
            codex_route.insert(category.to_owned(), route);
        }
        if let Some(category) = name.strip_prefix("codex_tier_") {
            codex_tier.insert(category.to_owned(), decode(entry["value"].clone(), name)?);
        }
    }
    Ok(Defaults {
        codex_route,
        codex_tier,
        codex_eta_max_min: field(&defaults_json, "codex_eta_max_min")?,
        codex_files_max: field(&defaults_json, "codex_files_max")?,
    })
}

pub fn parse(orchestration: &str, catalog: &str, defaults: &str) -> Result<Policy, String> {
    let document = orchestration
        .parse::<DocumentMut>()
        .map_err(|e| format!("orchestration: {e}"))?;
    let config = json_value(document.as_item());
    let router: RouterPolicy = decode(config["router"].clone(), "router").map_err(|error| {
        if error.contains("unknown variant") {
            format!("router.order: {error}")
        } else {
            error
        }
    })?;
    if router.source.trim().is_empty() {
        return Err("router.source: must not be empty".into());
    }
    if router.order.is_empty() {
        return Err("router.order: must not be empty".into());
    }
    let mut seen = BTreeSet::new();
    for value in &router.order {
        if !seen.insert(*value) {
            return Err(format!("router.order: duplicate value {value:?}"));
        }
    }
    let catalog = catalog
        .parse::<DocumentMut>()
        .map_err(|e| format!("catalog: {e}"))?;
    let catalog = json_value(catalog.as_item());
    let defaults = parse_defaults(defaults)?;
    Ok(Policy {
        router,
        mode: decode(config["mode"].clone(), "mode")?,
        accounts: decode(config["accounts"].clone(), "accounts")?,
        projects: decode(config["projects"].clone(), "projects")?,
        tiers: tiers(&document, "tiers")?,
        on_request: tiers(&document, "on_request")?,
        hosts: decode(config["hosts"].clone(), "hosts")?,
        catalog: decode(catalog["accounts"].clone(), "catalog.accounts")?,
        defaults,
    })
}
